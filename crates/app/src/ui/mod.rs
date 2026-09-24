//! Application shell: actions, layout, input routing and views.

pub mod actions;
mod app;
mod commands;
mod events;
pub mod input;
pub mod layout;
pub mod palette;
pub mod settings;
mod state;
pub mod style;
mod view;

pub use app::{App, AppEvent, Message, Options};
pub use commands::INITIAL_WINDOW;
