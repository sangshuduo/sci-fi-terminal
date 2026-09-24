//! Compile-time panel registry.
//!
//! Panels are trusted built-in Rust components. They receive a scoped,
//! read-only [`PanelContext`] — never terminal models, PTY handles or the
//! ability to inject input — so adding a panel never touches terminal
//! internals. Panels may emit [`PanelMsg`]s; the app decides what they do.

pub mod files;
pub mod metrics;
mod views;
pub mod worker;

use iced::Element;

pub use files::{FilesMsg, FilesState};
pub use metrics::{MetricsSample, format_bytes};
pub use worker::{ConnectionView, HomeLocation, MonitorPlan, MonitorSample, MonitorWorker};

/// Read-only services a panel may use.
pub struct PanelContext<'a> {
    pub sample: &'a MonitorSample,
    /// Show the top-process table inside the System panel.
    pub show_processes: bool,
    /// Globe drawing inputs for the network panel; `None` hides the globe.
    pub globe: Option<crate::render::globe::GlobeView<'a>>,
}

/// Messages panels can emit.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelMsg {
    Files(FilesMsg),
}

/// Which side of the terminal area a panel docks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dock {
    Left,
    Right,
}

/// The registered built-in panels. Each is toggled independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinPanel {
    /// CPU, memory, swap and top processes.
    System,
    /// Peer globe, interface rates and connections.
    Network,
    /// Directory viewer following the focused shell.
    Directory,
}

impl BuiltinPanel {
    pub const ALL: [BuiltinPanel; 3] = [Self::System, Self::Network, Self::Directory];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Network => "network",
            Self::Directory => "directory",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Network => "Network",
            Self::Directory => "Directory",
        }
    }

    pub fn dock(self) -> Dock {
        match self {
            Self::Directory => Dock::Left,
            Self::System | Self::Network => Dock::Right,
        }
    }

    pub fn view<'a, M: Clone + 'a>(
        self,
        context: &PanelContext<'a>,
        wrap: impl Fn(PanelMsg) -> M + Copy + 'a,
    ) -> Element<'a, M> {
        let sample = context.sample;
        match self {
            Self::System => views::system(sample, context.show_processes),
            Self::Network => views::network(sample, context.globe.as_ref()),
            Self::Directory => match &sample.files {
                Some(state) => files::view(state, move |m| wrap(PanelMsg::Files(m))),
                None => views::placeholder("No focused shell"),
            },
        }
    }
}

#[derive(Default)]
pub struct PanelRegistry {
    panels: Vec<BuiltinPanel>,
}

impl PanelRegistry {
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for panel in BuiltinPanel::ALL {
            registry.register(panel);
        }
        registry
    }

    /// Register a panel; duplicate ids are ignored so registration is idempotent.
    pub fn register(&mut self, panel: BuiltinPanel) -> bool {
        if self.get(panel.id()).is_some() {
            return false;
        }
        self.panels.push(panel);
        true
    }

    pub fn get(&self, id: &str) -> Option<BuiltinPanel> {
        self.panels.iter().copied().find(|panel| panel.id() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = BuiltinPanel> + '_ {
        self.panels.iter().copied()
    }

    /// Registered panels docked on `dock`, in registration order.
    pub fn docked(&self, dock: Dock) -> impl Iterator<Item = BuiltinPanel> + '_ {
        self.iter().filter(move |panel| panel.dock() == dock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_has_three_independent_panels() {
        let registry = PanelRegistry::builtin();
        let ids: Vec<_> = registry.iter().map(BuiltinPanel::id).collect();
        assert_eq!(ids, vec!["system", "network", "directory"]);
        let right: Vec<_> = registry.docked(Dock::Right).collect();
        assert_eq!(right, vec![BuiltinPanel::System, BuiltinPanel::Network]);
        let left: Vec<_> = registry.docked(Dock::Left).collect();
        assert_eq!(left, vec![BuiltinPanel::Directory]);
    }

    #[test]
    fn duplicate_registration_is_ignored() {
        let mut registry = PanelRegistry::builtin();
        assert!(!registry.register(BuiltinPanel::Network));
        assert_eq!(registry.iter().count(), 3);
    }

    #[test]
    fn panels_render_from_scoped_context_only() {
        let sample = MonitorSample::default();
        let context = PanelContext {
            sample: &sample,
            show_processes: true,
            globe: None,
        };
        for panel in PanelRegistry::builtin().iter() {
            let _element: Element<'_, PanelMsg> = panel.view(&context, |m| m);
        }
    }
}
