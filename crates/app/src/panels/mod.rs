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
pub use worker::{ConnectionView, MonitorPlan, MonitorSample, MonitorWorker};

/// Read-only services a panel may use.
pub struct PanelContext<'a> {
    pub sample: &'a MonitorSample,
    pub session_count: usize,
    pub live_sessions: usize,
}

/// Messages panels can emit.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelMsg {
    Files(FilesMsg),
}

/// Identifies what a panel needs sampled while it is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Nothing,
    System,
    Processes,
    Network,
    Files,
}

/// The registered built-in panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinPanel {
    System,
    Processes,
    Network,
    Files,
    Sessions,
}

impl BuiltinPanel {
    pub fn id(self) -> &'static str {
        match self {
            Self::System => "metrics",
            Self::Processes => "processes",
            Self::Network => "network",
            Self::Files => "files",
            Self::Sessions => "sessions",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Processes => "Processes",
            Self::Network => "Network",
            Self::Files => "Directory",
            Self::Sessions => "Sessions",
        }
    }

    pub fn needs(self) -> Needs {
        match self {
            Self::System => Needs::System,
            Self::Processes => Needs::Processes,
            Self::Network => Needs::Network,
            Self::Files => Needs::Files,
            Self::Sessions => Needs::Nothing,
        }
    }

    pub fn view<'a, M: Clone + 'a>(
        self,
        context: &PanelContext<'a>,
        wrap: impl Fn(PanelMsg) -> M + Copy + 'a,
    ) -> Element<'a, M> {
        let sample = context.sample;
        match self {
            Self::System => views::system(sample.system.as_ref()),
            Self::Processes => views::processes(&sample.processes),
            Self::Network => views::network(sample),
            Self::Files => match &sample.files {
                Some(state) => files::view(state, move |m| wrap(PanelMsg::Files(m))),
                None => views::placeholder("No focused shell"),
            },
            Self::Sessions => views::sessions(context.session_count, context.live_sessions),
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
        for panel in [
            BuiltinPanel::System,
            BuiltinPanel::Processes,
            BuiltinPanel::Network,
            BuiltinPanel::Files,
            BuiltinPanel::Sessions,
        ] {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_lists_panels_in_order() {
        let registry = PanelRegistry::builtin();
        let ids: Vec<_> = registry.iter().map(BuiltinPanel::id).collect();
        assert_eq!(
            ids,
            vec!["metrics", "processes", "network", "files", "sessions"]
        );
        assert_eq!(
            registry.get("network").map(BuiltinPanel::needs),
            Some(Needs::Network)
        );
    }

    #[test]
    fn duplicate_registration_is_ignored() {
        let mut registry = PanelRegistry::builtin();
        assert!(!registry.register(BuiltinPanel::Sessions));
        assert_eq!(registry.iter().count(), 5);
    }

    #[test]
    fn panels_render_from_scoped_context_only() {
        let sample = MonitorSample::default();
        let context = PanelContext {
            sample: &sample,
            session_count: 2,
            live_sessions: 1,
        };
        for panel in PanelRegistry::builtin().iter() {
            let _element: Element<'_, PanelMsg> = panel.view(&context, |m| m);
        }
    }
}
