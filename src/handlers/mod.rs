//! # Handlers Module
//!
//! This module contains command handlers for processing Discord slash commands and interactions.
//! Each submodule handles a specific category of commands related to different aspects of the application.
//!
//! ## Key Components
//!
//! - `dashboard`: Handlers for dashboard-related commands
//! - `queue`: Handlers for queue management commands
//! - `session`: Handlers for session management commands
//! - `admin`: Handlers for administrative commands
//! - `role`: Handlers for role management commands

pub mod admin;
pub mod dashboard;
pub mod queue;
pub mod role;
pub mod session;

pub use admin::*;
pub use dashboard::*;
pub use queue::*;
pub use role::*;
pub use session::*;
