//! # Discord Module
//!
//! This module contains Discord-specific code and utilities.
//! It provides an abstraction layer between the core application logic
//! and the Discord API.

mod handler;
mod commands;
mod utils;

pub use handler::*;
pub use commands::*;
pub use utils::*;
