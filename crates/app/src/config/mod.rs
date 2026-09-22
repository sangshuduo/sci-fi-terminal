//! Typed configuration: schema, validation, layered loading and themes.

pub mod load;
pub mod schema;
pub mod theme;
pub mod validate;

#[cfg(test)]
mod tests;

pub use load::{
    ConfigError, ConfigPaths, LoadedConfig, MAX_CONFIG_BYTES, load_effective, load_file,
    parse_config, save_overrides, write_atomic,
};
pub use schema::*;
pub use theme::{
    Rgb, TerminalPalette, Theme, UiColors, builtin_themes, contrast_ratio, find_theme,
    load_user_themes, parse_theme,
};
pub use validate::{Diagnostic, validate};
