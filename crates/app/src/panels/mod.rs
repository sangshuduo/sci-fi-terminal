//! Compile-time panel registry.
//!
//! Panels are trusted built-in Rust components. They receive a scoped,
//! read-only [`PanelContext`] — never terminal models, PTY handles or the
//! ability to inject input — so adding a panel never touches terminal internals.

pub mod metrics;

use iced::widget::{column, container, progress_bar, row, text};
use iced::{Element, Length};

pub use metrics::{MetricsSample, MetricsWorker, format_bytes};

/// Read-only services a panel may use.
pub struct PanelContext<'a> {
    pub metrics: Option<&'a MetricsSample>,
    pub session_count: usize,
    pub live_sessions: usize,
}

/// A built-in side panel.
pub trait Panel {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// Whether the metrics worker should run while this panel is visible.
    fn wants_metrics(&self) -> bool {
        false
    }
    fn view<'a, M: 'a>(&self, context: &PanelContext<'a>) -> Element<'a, M>;
}

/// The registered panels, in display order.
pub enum BuiltinPanel {
    Metrics(MetricsPanel),
    Sessions(SessionsPanel),
}

impl BuiltinPanel {
    fn inner(&self) -> &dyn PanelMeta {
        match self {
            Self::Metrics(panel) => panel,
            Self::Sessions(panel) => panel,
        }
    }

    pub fn id(&self) -> &'static str {
        self.inner().meta_id()
    }

    pub fn title(&self) -> &'static str {
        self.inner().meta_title()
    }

    pub fn wants_metrics(&self) -> bool {
        matches!(self, Self::Metrics(_))
    }

    pub fn view<'a, M: 'a>(&self, context: &PanelContext<'a>) -> Element<'a, M> {
        match self {
            Self::Metrics(panel) => panel.view(context),
            Self::Sessions(panel) => panel.view(context),
        }
    }
}

/// Object-safe metadata view of a panel.
trait PanelMeta {
    fn meta_id(&self) -> &'static str;
    fn meta_title(&self) -> &'static str;
}

impl<P: Panel> PanelMeta for P {
    fn meta_id(&self) -> &'static str {
        self.id()
    }
    fn meta_title(&self) -> &'static str {
        self.title()
    }
}

#[derive(Default)]
pub struct PanelRegistry {
    panels: Vec<BuiltinPanel>,
}

impl PanelRegistry {
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        registry.register(BuiltinPanel::Metrics(MetricsPanel));
        registry.register(BuiltinPanel::Sessions(SessionsPanel));
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

    pub fn get(&self, id: &str) -> Option<&BuiltinPanel> {
        self.panels.iter().find(|panel| panel.id() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &BuiltinPanel> {
        self.panels.iter()
    }
}

/// CPU and memory overview.
pub struct MetricsPanel;

impl Panel for MetricsPanel {
    fn id(&self) -> &'static str {
        "metrics"
    }

    fn title(&self) -> &'static str {
        "System"
    }

    fn wants_metrics(&self) -> bool {
        true
    }

    fn view<'a, M: 'a>(&self, context: &PanelContext<'a>) -> Element<'a, M> {
        let Some(sample) = context.metrics else {
            return text("Sampling…").size(13).into();
        };
        let mut cores = column![].spacing(3);
        for (index, usage) in sample
            .per_core
            .iter()
            .take(metrics::MAX_CORES_SHOWN)
            .enumerate()
        {
            cores = cores.push(
                row![
                    text(format!("{index:>2}"))
                        .size(11)
                        .width(Length::Fixed(20.0)),
                    progress_bar(0.0..=100.0, *usage).girth(6),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            );
        }
        if sample.per_core.len() > metrics::MAX_CORES_SHOWN {
            cores = cores.push(
                text(format!(
                    "+{} more cores",
                    sample.per_core.len() - metrics::MAX_CORES_SHOWN
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
}

/// Session counts: a second built-in proving registration needs no terminal access.
pub struct SessionsPanel;

impl Panel for SessionsPanel {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn title(&self) -> &'static str {
        "Sessions"
    }

    fn view<'a, M: 'a>(&self, context: &PanelContext<'a>) -> Element<'a, M> {
        column![
            labelled("Open", context.session_count.to_string()),
            labelled("Running", context.live_sessions.to_string()),
        ]
        .spacing(6)
        .into()
    }
}

fn labelled<'a, M: 'a>(label: &'a str, value: String) -> Element<'a, M> {
    container(
        row![
            text(label).size(13).width(Length::Fill),
            text(value).size(13)
        ]
        .spacing(8),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_lists_panels_in_order() {
        let registry = PanelRegistry::builtin();
        let ids: Vec<_> = registry.iter().map(BuiltinPanel::id).collect();
        assert_eq!(ids, vec!["metrics", "sessions"]);
        assert!(
            registry
                .get("metrics")
                .is_some_and(BuiltinPanel::wants_metrics)
        );
        assert!(
            !registry
                .get("sessions")
                .is_some_and(BuiltinPanel::wants_metrics)
        );
    }

    #[test]
    fn duplicate_registration_is_ignored() {
        let mut registry = PanelRegistry::builtin();
        assert!(!registry.register(BuiltinPanel::Sessions(SessionsPanel)));
        assert_eq!(registry.iter().count(), 2);
    }

    #[test]
    fn panels_render_from_scoped_context_only() {
        let context = PanelContext {
            metrics: None,
            session_count: 2,
            live_sessions: 1,
        };
        for panel in PanelRegistry::builtin().iter() {
            let _element: Element<'_, ()> = panel.view(&context);
        }
    }
}
