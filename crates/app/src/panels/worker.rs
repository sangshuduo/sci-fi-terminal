//! One low-rate background worker for every monitoring panel.
//!
//! The UI sends a [`MonitorPlan`] describing what is visible; the worker
//! samples only that, at the requested cadence, and delivers a
//! [`MonitorSample`]. Nothing here runs on, or blocks, the GUI thread.

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::files::{FilesState, list_directory};
use super::metrics::{MIN_INTERVAL, MetricsSample, SystemSampler};
use crate::monitor::{
    Connection, GeoIp, GeoLocation, InterfaceRate, NetworkSampler, ProcessInfo, ProcessSampler,
    list_connections, public_ip,
};

/// What the visible panels need. `None` disables that collector entirely.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonitorPlan {
    pub interval: Duration,
    pub system: bool,
    pub processes: Option<usize>,
    /// Network cadence; interfaces and connections are sampled at most this often.
    pub network: Option<Duration>,
    pub connections: bool,
    pub geoip_database: Option<PathBuf>,
    /// Opt-in public IP endpoint (ADR-007); `None` means no network request.
    pub public_ip_endpoint: Option<String>,
    /// Shell pid whose working directory the file viewer follows.
    pub files_pid: Option<u32>,
    pub show_hidden: bool,
}

impl MonitorPlan {
    pub fn is_idle(&self) -> bool {
        !self.system
            && self.processes.is_none()
            && self.network.is_none()
            && self.files_pid.is_none()
    }
}

/// Retry delay after a failed public IP lookup.
const HOME_RETRY: Duration = Duration::from_secs(5 * 60);

/// This machine's public address and its GeoIP location, if known.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeLocation {
    pub ip: IpAddr,
    pub location: Option<GeoLocation>,
}

impl HomeLocation {
    /// Short label such as `Toronto, CA`.
    pub fn place(&self) -> Option<String> {
        let loc = self.location.as_ref()?;
        let country = loc.country_code.as_deref().or(loc.country.as_deref());
        match (loc.city.as_deref(), country) {
            (Some(city), Some(country)) => Some(format!("{city}, {country}")),
            (Some(city), None) => Some(city.to_owned()),
            (None, Some(country)) => Some(country.to_owned()),
            (None, None) => None,
        }
    }
}

/// A connection plus its optional offline GeoIP result.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionView {
    pub connection: Connection,
    pub location: Option<GeoLocation>,
}

/// Everything sampled in one tick. Fields not in the plan keep their last value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonitorSample {
    pub system: Option<MetricsSample>,
    pub processes: Vec<ProcessInfo>,
    pub interfaces: Vec<InterfaceRate>,
    pub connections: Option<Result<Vec<ConnectionView>, String>>,
    pub geoip_status: Option<String>,
    /// This machine's public address and, with a GeoIP database, its city.
    pub home: Option<HomeLocation>,
    pub home_status: Option<String>,
    pub files: Option<FilesState>,
}

enum Control {
    Plan(MonitorPlan),
    Stop,
}

/// Owns the worker thread; dropping it stops sampling promptly.
pub struct MonitorWorker {
    control: Sender<Control>,
    thread: Option<JoinHandle<()>>,
}

impl MonitorWorker {
    pub fn start(
        plan: MonitorPlan,
        sink: Arc<dyn Fn(MonitorSample) + Send + Sync>,
    ) -> Option<Self> {
        let (control, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("monitor".into())
            .stack_size(512 * 1024)
            .spawn(move || Collector::new(plan).run(&rx, sink.as_ref()))
            .ok()?;
        Some(Self {
            control,
            thread: Some(thread),
        })
    }

    pub fn set_plan(&self, plan: MonitorPlan) {
        let _ = self.control.send(Control::Plan(plan));
    }
}

impl Drop for MonitorWorker {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Stop);
        // The worker wakes immediately on Stop; a sample in progress is bounded.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Collector {
    plan: MonitorPlan,
    system: Option<SystemSampler>,
    processes: Option<ProcessSampler>,
    network: Option<NetworkSampler>,
    geoip: Option<(PathBuf, Result<GeoIp, String>)>,
    /// Last public IP result, the endpoint it came from, and when to retry.
    public_ip: Option<(String, Result<IpAddr, String>, Instant)>,
    last_network: Option<Instant>,
    sample: MonitorSample,
}

impl Collector {
    fn new(plan: MonitorPlan) -> Self {
        Self {
            plan,
            system: None,
            processes: None,
            network: None,
            geoip: None,
            public_ip: None,
            last_network: None,
            sample: MonitorSample::default(),
        }
    }

    fn run(mut self, control: &Receiver<Control>, sink: &dyn Fn(MonitorSample)) {
        // Sample immediately so panels fill in without waiting a full interval.
        self.tick();
        sink(self.sample.clone());
        loop {
            match control.recv_timeout(self.plan.interval.max(MIN_INTERVAL)) {
                Ok(Control::Plan(plan)) => {
                    let files_changed = plan.files_pid != self.plan.files_pid
                        || plan.show_hidden != self.plan.show_hidden;
                    self.plan = plan;
                    if !files_changed {
                        continue;
                    }
                }
                Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.tick();
            sink(self.sample.clone());
        }
    }

    fn tick(&mut self) {
        if self.plan.system {
            let sampler = self.system.get_or_insert_with(SystemSampler::new);
            self.sample.system = Some(sampler.sample());
        }
        if let Some(limit) = self.plan.processes {
            let sampler = self.processes.get_or_insert_with(ProcessSampler::new);
            self.sample.processes = sampler.sample(limit);
        }
        self.tick_network();
        self.tick_files();
    }

    fn tick_network(&mut self) {
        let Some(every) = self.plan.network else {
            return;
        };
        if self
            .last_network
            .is_some_and(|at| at.elapsed() + Duration::from_millis(50) < every)
        {
            return;
        }
        self.last_network = Some(Instant::now());
        self.sample.interfaces = self
            .network
            .get_or_insert_with(NetworkSampler::new)
            .sample();
        self.refresh_geoip();
        self.tick_home();
        if !self.plan.connections {
            self.sample.connections = None;
            return;
        }
        let listed = list_connections();
        let geoip = self.geoip.as_mut().and_then(|(_, db)| db.as_mut().ok());
        self.sample.connections = Some(listed.map(|list| locate(list, geoip)));
    }

    /// Opt-in public IP lookup: at most every 30 min (5 min after a failure),
    /// and immediately when the endpoint changes. Disabled → no request.
    fn tick_home(&mut self) {
        let Some(endpoint) = self.plan.public_ip_endpoint.clone() else {
            self.public_ip = None;
            self.sample.home = None;
            self.sample.home_status = None;
            return;
        };
        let due = match &self.public_ip {
            Some((last, _, retry_at)) => *last != endpoint || Instant::now() >= *retry_at,
            None => true,
        };
        if due {
            let result = public_ip::fetch(&endpoint);
            let wait = if result.is_ok() {
                public_ip::REFRESH_INTERVAL
            } else {
                HOME_RETRY
            };
            self.public_ip = Some((endpoint, result, Instant::now() + wait));
        }
        let Some((_, result, _)) = &self.public_ip else {
            return;
        };
        match result {
            Ok(ip) => {
                let ip = *ip;
                let geoip = self.geoip.as_mut().and_then(|(_, db)| db.as_mut().ok());
                let location = geoip.and_then(|db| db.lookup(ip));
                self.sample.home_status = location
                    .is_none()
                    .then(|| "Your city needs a GeoIP database (Settings → Panels)".to_owned());
                self.sample.home = Some(HomeLocation { ip, location });
            }
            Err(err) => {
                self.sample.home = None;
                self.sample.home_status = Some(err.clone());
            }
        }
    }

    /// Open (or re-open after a path change) the user's offline database.
    fn refresh_geoip(&mut self) {
        let wanted = self.plan.geoip_database.clone();
        let current = self.geoip.as_ref().map(|(path, _)| path.clone());
        if wanted == current {
            return;
        }
        self.geoip = wanted.map(|path| {
            let db = GeoIp::open(&path).map_err(|err| err.to_string());
            (path, db)
        });
        self.sample.geoip_status = self.geoip.as_ref().map(|(path, db)| match db {
            Ok(_) => format!("GeoIP: {}", path.display()),
            Err(err) => err.clone(),
        });
    }

    fn tick_files(&mut self) {
        let Some(pid) = self.plan.files_pid else {
            self.sample.files = None;
            return;
        };
        let sampler = self.processes.get_or_insert_with(ProcessSampler::new);
        self.sample.files = Some(match sampler.cwd_of(pid) {
            Some(cwd) => match list_directory(&cwd, self.plan.show_hidden) {
                Ok(listing) => FilesState::Listed(listing),
                Err(err) => FilesState::Error(err),
            },
            None => FilesState::Unknown,
        });
    }
}

fn locate(list: Vec<Connection>, mut geoip: Option<&mut GeoIp>) -> Vec<ConnectionView> {
    list.into_iter()
        .map(|connection| {
            let remote: Option<IpAddr> = connection.remote.map(|addr| addr.ip());
            let location = match (remote, geoip.as_deref_mut()) {
                (Some(ip), Some(db)) => db.lookup(ip),
                _ => None,
            };
            ConnectionView {
                connection,
                location,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn collect(plan: MonitorPlan) -> MonitorSample {
        let got = Arc::new(Mutex::new(None));
        let sink = {
            let got = got.clone();
            Arc::new(move |s: MonitorSample| *got.lock().expect("lock") = Some(s))
        };
        let worker = MonitorWorker::start(plan, sink).expect("start");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(sample) = got.lock().expect("lock").clone() {
                drop(worker);
                return sample;
            }
            assert!(Instant::now() < deadline, "no sample");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn samples_only_what_the_plan_requests() {
        let plan = MonitorPlan {
            interval: MIN_INTERVAL,
            system: true,
            ..MonitorPlan::default()
        };
        let sample = collect(plan);
        assert!(sample.system.is_some());
        assert!(sample.processes.is_empty());
        assert!(sample.connections.is_none());
        assert!(sample.files.is_none());
    }

    #[test]
    fn follows_this_process_working_directory() {
        let plan = MonitorPlan {
            interval: MIN_INTERVAL,
            files_pid: Some(std::process::id()),
            processes: Some(3),
            ..MonitorPlan::default()
        };
        let sample = collect(plan);
        assert!(sample.processes.len() <= 3);
        match sample.files {
            Some(FilesState::Listed(listing)) => {
                assert_eq!(listing.path, std::env::current_dir().expect("cwd"));
            }
            // Some platforms do not expose another process's cwd; that must not fail.
            Some(FilesState::Unknown | FilesState::Error(_)) => {}
            None => panic!("files requested but not sampled"),
        }
    }

    #[test]
    fn missing_geoip_database_reports_status_without_network() {
        let plan = MonitorPlan {
            interval: MIN_INTERVAL,
            network: Some(MIN_INTERVAL),
            connections: true,
            geoip_database: Some(PathBuf::from("/definitely/not/here.mmdb")),
            ..MonitorPlan::default()
        };
        let sample = collect(plan);
        assert!(
            sample
                .geoip_status
                .as_deref()
                .is_some_and(|s| s.contains("could not be opened"))
        );
    }

    #[test]
    fn idle_plan_detection() {
        assert!(MonitorPlan::default().is_idle());
        assert!(
            !MonitorPlan {
                system: true,
                ..MonitorPlan::default()
            }
            .is_idle()
        );
    }

    #[test]
    fn disabled_public_ip_lookup_makes_no_request_and_reports_nothing() {
        let plan = MonitorPlan {
            interval: MIN_INTERVAL,
            network: Some(MIN_INTERVAL),
            public_ip_endpoint: None,
            ..MonitorPlan::default()
        };
        let sample = collect(plan);
        assert!(sample.home.is_none() && sample.home_status.is_none());
    }

    /// Live check of the opt-in lookup. Makes a real HTTPS request, so it only
    /// runs on demand: `SFT_GEOIP_DB=/path/city.mmdb cargo test -- --ignored`.
    #[test]
    #[ignore = "network: sends one request to the public IP endpoint"]
    fn live_public_ip_is_located_with_a_real_database() {
        let Ok(db) = std::env::var("SFT_GEOIP_DB") else {
            eprintln!("SFT_GEOIP_DB not set; skipping");
            return;
        };
        let plan = MonitorPlan {
            interval: MIN_INTERVAL,
            network: Some(MIN_INTERVAL),
            geoip_database: Some(PathBuf::from(db)),
            public_ip_endpoint: Some(public_ip::DEFAULT_ENDPOINT.to_owned()),
            ..MonitorPlan::default()
        };
        let sample = collect(plan);
        let home = sample.home.expect("public IP resolved");
        let place = home.place().expect("city located");
        let location = home.location.expect("location");
        eprintln!(
            "home: {place} lat={:?} lon={:?}",
            location.latitude, location.longitude
        );
        assert!(location.latitude.is_some() && location.longitude.is_some());
    }
}
