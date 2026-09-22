//! Versioned layout tree: tabs of horizontal/vertical splits with ratios.
//!
//! Only geometry and safe profile identifiers are persisted — never screen
//! contents, commands, environment or executable overrides. Restoring creates
//! fresh sessions under the named profiles.

use std::path::Path;

use iced::widget::pane_grid::{self, Axis, Configuration, Node};
use serde::{Deserialize, Serialize};

use crate::config::{ConfigError, MAX_CONFIG_BYTES, write_atomic};

pub const LAYOUT_VERSION: u32 = 1;
pub const MIN_RATIO: f32 = 0.1;
pub const MAX_RATIO: f32 = 0.9;
/// Visible terminal panes per tab.
pub const MAX_PANES_PER_TAB: usize = 4;

pub fn clamp_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(MIN_RATIO, MAX_RATIO)
    } else {
        0.5
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitAxis {
    /// Side by side (a vertical divider).
    Vertical,
    /// Stacked (a horizontal divider).
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum LayoutNode {
    Pane {
        profile: String,
    },
    Split {
        axis: SplitAxis,
        ratio: f32,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn pane_count(&self) -> usize {
        match self {
            Self::Pane { .. } => 1,
            Self::Split { a, b, .. } => a.pane_count() + b.pane_count(),
        }
    }

    fn depth(&self) -> usize {
        match self {
            Self::Pane { .. } => 1,
            Self::Split { a, b, .. } => 1 + a.depth().max(b.depth()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabLayout {
    pub root: LayoutNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutFile {
    pub schema_version: u32,
    #[serde(default)]
    pub active_tab: usize,
    pub tabs: Vec<TabLayout>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LayoutError {
    #[error("layout: unsupported schema_version {0}")]
    Version(u32),
    #[error("layout: {0}")]
    Invalid(String),
}

impl LayoutFile {
    /// Single tab, single pane with the default profile.
    pub fn focus(profile: &str) -> Self {
        Self {
            schema_version: LAYOUT_VERSION,
            active_tab: 0,
            tabs: vec![TabLayout {
                root: LayoutNode::Pane {
                    profile: profile.into(),
                },
            }],
        }
    }

    /// One tab split side by side.
    pub fn split(profile: &str) -> Self {
        let pane = || {
            Box::new(LayoutNode::Pane {
                profile: profile.into(),
            })
        };
        Self {
            schema_version: LAYOUT_VERSION,
            active_tab: 0,
            tabs: vec![TabLayout {
                root: LayoutNode::Split {
                    axis: SplitAxis::Vertical,
                    ratio: 0.5,
                    a: pane(),
                    b: pane(),
                },
            }],
        }
    }

    /// Check structural limits so a hand-edited file cannot request unbounded sessions.
    pub fn validate(&self, max_sessions: usize) -> Result<(), LayoutError> {
        if self.schema_version != LAYOUT_VERSION {
            return Err(LayoutError::Version(self.schema_version));
        }
        if self.tabs.is_empty() {
            return Err(LayoutError::Invalid("no tabs".into()));
        }
        let total: usize = self.tabs.iter().map(|tab| tab.root.pane_count()).sum();
        if total > max_sessions {
            return Err(LayoutError::Invalid(format!(
                "{total} panes exceed the {max_sessions} session limit"
            )));
        }
        for tab in &self.tabs {
            if tab.root.pane_count() > MAX_PANES_PER_TAB || tab.root.depth() > MAX_PANES_PER_TAB {
                return Err(LayoutError::Invalid(format!(
                    "a tab has more than {MAX_PANES_PER_TAB} panes"
                )));
            }
        }
        Ok(())
    }

    pub fn load(path: &Path, max_sessions: usize) -> Result<Option<Self>, String> {
        let meta = match std::fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(format!("{}: {err}", path.display())),
        };
        if meta.len() > MAX_CONFIG_BYTES {
            return Err(format!("{}: file exceeds 1 MiB limit", path.display()));
        }
        let text =
            std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        let file: Self =
            toml::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
        file.validate(max_sessions)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        Ok(Some(file))
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = toml::to_string_pretty(self).map_err(|err| ConfigError::Parse {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        write_atomic(path, &text)
    }
}

/// Capture a pane grid's geometry; `profile_of` maps pane state to a profile id.
pub fn capture<T>(state: &pane_grid::State<T>, profile_of: impl Fn(&T) -> String) -> LayoutNode {
    capture_node(state.layout(), state, &profile_of)
}

fn capture_node<T>(
    node: &Node,
    state: &pane_grid::State<T>,
    profile_of: &impl Fn(&T) -> String,
) -> LayoutNode {
    match node {
        Node::Pane(pane) => LayoutNode::Pane {
            profile: state
                .get(*pane)
                .map(profile_of)
                .unwrap_or_else(|| "default".into()),
        },
        Node::Split {
            axis, ratio, a, b, ..
        } => LayoutNode::Split {
            axis: match axis {
                Axis::Vertical => SplitAxis::Vertical,
                Axis::Horizontal => SplitAxis::Horizontal,
            },
            ratio: clamp_ratio(*ratio),
            a: Box::new(capture_node(a, state, profile_of)),
            b: Box::new(capture_node(b, state, profile_of)),
        },
    }
}

/// Build a pane-grid configuration, creating pane state for each leaf.
pub fn restore<T>(node: &LayoutNode, make_pane: &mut impl FnMut(&str) -> T) -> Configuration<T> {
    match node {
        LayoutNode::Pane { profile } => Configuration::Pane(make_pane(profile)),
        LayoutNode::Split { axis, ratio, a, b } => Configuration::Split {
            axis: match axis {
                SplitAxis::Vertical => Axis::Vertical,
                SplitAxis::Horizontal => Axis::Horizontal,
            },
            ratio: clamp_ratio(*ratio),
            a: Box::new(restore(a, make_pane)),
            b: Box::new(restore(b, make_pane)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_are_clamped() {
        assert_eq!(clamp_ratio(0.0), MIN_RATIO);
        assert_eq!(clamp_ratio(1.0), MAX_RATIO);
        assert_eq!(clamp_ratio(f32::NAN), 0.5);
        assert_eq!(clamp_ratio(0.3), 0.3);
    }

    #[test]
    fn round_trip_through_pane_grid() {
        let file = LayoutFile::split("default");
        let config = restore(&file.tabs[0].root, &mut |profile: &str| profile.to_owned());
        let state = pane_grid::State::with_configuration(config);
        assert_eq!(state.len(), 2);
        let captured = capture(&state, Clone::clone);
        assert_eq!(captured, file.tabs[0].root);
    }

    #[test]
    fn toml_round_trip_and_unknown_fields_rejected() {
        let file = LayoutFile::split("work");
        let text = toml::to_string_pretty(&file).expect("serialize");
        assert_eq!(toml::from_str::<LayoutFile>(&text).expect("parse"), file);
        let bad = text.replace("schema_version", "schema_version = 1\nextra");
        assert!(toml::from_str::<LayoutFile>(&bad).is_err());
    }

    #[test]
    fn limits_are_enforced() {
        let pane = || {
            Box::new(LayoutNode::Pane {
                profile: "default".into(),
            })
        };
        let split = |a, b| {
            Box::new(LayoutNode::Split {
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                a,
                b,
            })
        };
        let five = LayoutNode::Split {
            axis: SplitAxis::Vertical,
            ratio: 0.5,
            a: split(pane(), pane()),
            b: split(pane(), split(pane(), pane())),
        };
        let file = LayoutFile {
            schema_version: 1,
            active_tab: 0,
            tabs: vec![TabLayout { root: five }],
        };
        assert!(matches!(file.validate(8), Err(LayoutError::Invalid(_))));
        let many = LayoutFile {
            tabs: vec![TabLayout { root: *pane() }; 9],
            ..LayoutFile::focus("x")
        };
        assert!(matches!(many.validate(8), Err(LayoutError::Invalid(_))));
        let newer = LayoutFile {
            schema_version: 2,
            ..LayoutFile::focus("x")
        };
        assert_eq!(newer.validate(8), Err(LayoutError::Version(2)));
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("sft-layout-{}", std::process::id()));
        let path = dir.join("layout.toml");
        LayoutFile::split("default").save(&path).expect("save");
        assert_eq!(
            LayoutFile::load(&path, 8),
            Ok(Some(LayoutFile::split("default")))
        );
        assert_eq!(LayoutFile::load(&dir.join("missing.toml"), 8), Ok(None));
        let _ = std::fs::remove_dir_all(dir);
    }
}
