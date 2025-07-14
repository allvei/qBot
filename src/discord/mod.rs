//! # Discord Module
//!
//! This module contains Discord-specific code and utilities.
//! It provides an abstraction layer between the core application logic
//! and the Discord API.

pub mod commands;
pub mod handler;
pub mod utils;

pub use commands::*;
pub use handler::*;
pub use utils::*;
