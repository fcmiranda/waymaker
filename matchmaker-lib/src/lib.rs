#![allow(irrefutable_let_patterns)]

// event
pub mod action;
pub use action::{Action, Actions, SortOrder};
pub mod binds;
pub mod config;
mod config_types;
pub mod event;
pub mod frecency;

pub mod message;
pub mod render;
pub mod spinner;
pub mod ui;
// picker
pub mod nucleo;
pub mod preview;
mod selector;
pub use selector::Selector;
mod matchmaker;
pub use matchmaker::*;
pub mod cache;
pub mod tui;
pub mod walker;

// misc
mod aliases;
pub mod errors;
pub mod utils;
pub use aliases::*;
pub use errors::*;

pub mod noninteractive;

pub static MODE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
pub static ACTION_BOX_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
