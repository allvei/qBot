//! # Handlers Module
//!
//! This module contains command handlers for processing Discord slash commands and interactions.
//! Each submodule handles a specific category of commands related to different aspects of the application.
//!
//! ## Key Components
//!
//! - `queue`: Handlers for queue management commands
//! - `game`: Handlers for game management commands
//! - `admin`: Handlers for administrative commands

pub mod admin;
pub mod player;
pub mod game;