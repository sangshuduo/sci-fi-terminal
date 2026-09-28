//! Optional touch-friendly on-screen keyboard.
//!
//! Layouts are data-only TOML files (built-in `en-us` plus user files in
//! `<config>/keyboards/<id>.toml`). Pressing keys yields the same
//! [`crate::ui::input::KeyPress`] events as a physical keyboard, so the
//! existing routing and terminal encoding are reused unchanged.

mod layout;
mod state;
mod view;

#[cfg(test)]
mod tests;

pub use layout::{
    KeyAction, KeySpec, Layout, MAX_KEYS, Modifier, builtin_layout, load_user_layouts, parse_layout,
};
pub use state::{Keyboard, OskMessage};
pub use view::view;
