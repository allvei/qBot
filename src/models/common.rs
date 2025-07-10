//! # Common Types Module
//!
//! This module contains common types and enums used across multiple modules
//! to avoid circular dependencies.

use serde::{Deserialize, Serialize};

/// Team assignment for players in a session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Team {
    /// Red team
    Red,
    /// Blue team
    Blue,
}
