//! Read-only widget views for the monitoring panels.

use iced::widget::{column, progress_bar, row, text};
use iced::{Alignment, Element, Length};

use super::metrics::{MAX_CORES_SHOWN, MetricsSample, format_bytes};
use super::worker::{ConnectionView, MonitorSample};
use crate::monitor::{ConnectionState, InterfaceRate, ProcessInfo, Protocol};

/// Connections shown in the panel; the full capped list stays in the sample.
const SHOWN_CONNECTIONS: usize = 12;

pub(super) fn placeholder<'a, M: 'a>(message: &'a str) -> Element<'a, M> {
    text(message).size(12).into()
}

fn labelled<'a, M: 'a>(label: &'a str, value: String) -> Element<'a, M> {
    row![
        text(label).size(13).width(Length::Fill),
        text(value).size(13)
    ]
    .spacing(8)
    .into()
}

pub(super) fn system<'a, M: 'a>(sample: Option<&MetricsSample>) -> Element<'a, M> {
    let Some(sample) = sample else {
        return placeholder("Sampling…");
    };
    let mut cores = column![].spacing(3);
    for (index, usage) in sample.per_core.iter().take(MAX_CORES_SHOWN).enumerate() {
        cores = cores.push(
            row![
                text(format!("{index:>2}")).size(11).width(20),
                progress_bar(0.0..=100.0, *usage).girth(6)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
    }
    if sample.per_core.len() > MAX_CORES_SHOWN {
        cores = cores.push(
            text(format!(
                "+{} more cores",
                sample.per_core.len() - MAX_CORES_SHOWN
            ))
            .size(11),
        );
    }
    let memory = format!(
        "{} / {}",
        format_bytes(sample.memory_used),
        format_bytes(sample.memory_total)
    );
    let swap = format!(
        "{} / {}",
        format_bytes(sample.swap_used),
        format_bytes(sample.swap_total)
    );
    column![
        labelled("CPU", format!("{:.0}%", sample.cpu_percent)),
        progress_bar(0.0..=100.0, sample.cpu_percent).girth(8),
        cores,
        labelled("Memory", memory),
        progress_bar(0.0..=1.0, sample.memory_fraction()).girth(8),
        labelled("Swap", swap),
        progress_bar(0.0..=1.0, sample.swap_fraction()).girth(8),
    ]
    .spacing(8)
    .into()
}

pub(super) fn processes<'a, M: 'a>(list: &[ProcessInfo]) -> Element<'a, M> {
    if list.is_empty() {
        return placeholder("Sampling…");
    }
    let header = row![
        text("PID").size(11).width(48),
        text("Name").size(11).width(Length::Fill),
        text("CPU").size(11).width(44),
        text("Mem").size(11).width(64),
    ];
    list.iter()
        .fold(column![header].spacing(2), |col, p| {
            col.push(row![
                text(p.pid.to_string()).size(11).width(48),
                text(p.name.clone()).size(11).width(Length::Fill),
                text(format!("{:.0}%", p.cpu_percent)).size(11).width(44),
                text(format_bytes(p.memory_bytes)).size(11).width(64),
            ])
        })
        .into()
}

pub(super) fn network<'a, M: 'a>(sample: &MonitorSample) -> Element<'a, M> {
    let mut col = column![].spacing(6);
    if sample.interfaces.is_empty() {
        col = col.push(text("No active interfaces").size(12));
    }
    for iface in &sample.interfaces {
        col = col.push(interface(iface));
    }
    if let Some(status) = &sample.geoip_status {
        col = col.push(text(status.clone()).size(10));
    }
    match &sample.connections {
        None => {}
        Some(Err(err)) => col = col.push(text(err.clone()).size(11)),
        Some(Ok(list)) => col = col.push(connections(list)),
    }
    col.into()
}

fn interface<'a, M: 'a>(iface: &InterfaceRate) -> Element<'a, M> {
    let rate = |bytes: f64| format!("{}/s", format_bytes(bytes.max(0.0) as u64));
    column![
        text(iface.name.clone()).size(12),
        row![
            text(format!("↓ {}", rate(iface.rx_bytes_per_sec)))
                .size(11)
                .width(Length::Fill),
            text(format!("↑ {}", rate(iface.tx_bytes_per_sec))).size(11),
        ],
    ]
    .spacing(2)
    .into()
}

fn connections<'a, M: 'a>(list: &[ConnectionView]) -> Element<'a, M> {
    let established = list
        .iter()
        .filter(|c| c.connection.state == ConnectionState::Established)
        .count();
    let listening = list
        .iter()
        .filter(|c| c.connection.state == ConnectionState::Listen)
        .count();
    let summary = text(format!("{established} established · {listening} listening")).size(11);
    list.iter()
        .filter(|c| c.connection.remote.is_some())
        .take(SHOWN_CONNECTIONS)
        .fold(column![summary].spacing(2), |col, view| {
            col.push(text(describe(view)).size(10))
        })
        .into()
}

/// One line per connection: protocol, remote endpoint, optional location.
pub(super) fn describe(view: &ConnectionView) -> String {
    let protocol = match view.connection.protocol {
        Protocol::Tcp => "tcp",
        Protocol::Udp => "udp",
    };
    let remote = view
        .connection
        .remote
        .map_or_else(|| "—".to_owned(), |addr| addr.to_string());
    let place = view.location.as_ref().and_then(|loc| {
        let city = loc.city.as_deref();
        let country = loc.country_code.as_deref().or(loc.country.as_deref());
        match (city, country) {
            (Some(city), Some(country)) => Some(format!("{city}, {country}")),
            (None, Some(country)) => Some(country.to_owned()),
            (Some(city), None) => Some(city.to_owned()),
            (None, None) => None,
        }
    });
    match place {
        Some(place) => format!("{protocol} {remote} · {place}"),
        None => format!("{protocol} {remote}"),
    }
}

pub(super) fn sessions<'a, M: 'a>(open: usize, running: usize) -> Element<'a, M> {
    column![
        labelled("Open", open.to_string()),
        labelled("Running", running.to_string())
    ]
    .spacing(6)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{Connection, GeoLocation};

    fn view(location: Option<GeoLocation>) -> ConnectionView {
        ConnectionView {
            connection: Connection {
                protocol: Protocol::Tcp,
                local: "10.0.0.2:5000".parse().expect("addr"),
                remote: Some("93.184.216.34:443".parse().expect("addr")),
                state: ConnectionState::Established,
                pids: vec![42],
            },
            location,
        }
    }

    #[test]
    fn describes_connections_with_and_without_location() {
        assert_eq!(describe(&view(None)), "tcp 93.184.216.34:443");
        let located = GeoLocation {
            country_code: Some("US".into()),
            country: Some("United States".into()),
            city: Some("Norwell".into()),
            latitude: None,
            longitude: None,
        };
        assert_eq!(
            describe(&view(Some(located))),
            "tcp 93.184.216.34:443 · Norwell, US"
        );
    }
}
