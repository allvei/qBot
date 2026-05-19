//! Database row mapping helpers to reduce repetitive code
//!
//! This module provides traits and helper functions for mapping SQL rows to structs,
//! eliminating boilerplate code and improving type safety.

use anyhow::Result;
use serenity::all::{GuildId as GI, RoleId};
use sqlx::{sqlite::SqliteRow, Row};

use crate::Rank;

/// Trait for mapping SQL rows to structs with common field patterns
pub trait RowMapper {
  /// Map a row to the implementing type
  fn from_row(row: &SqliteRow) -> Result<Self>
  where
    Self: Sized;
}

/// Helper functions for extracting common field patterns from SQL rows
pub struct RowHelpers;

impl RowHelpers {
  /// Extract ELO-related fields (elo, games, wins) with proper type conversion
  pub fn extract_elo_stats(row: &SqliteRow) -> Result<(u16, u32, u32)> {
    let elo: i64 = row.get("elo");
    let games: i64 = row.get("games");
    let wins: i64 = row.get("wins");

    Ok((elo as u16, games as u32, wins as u32))
  }

  /// Extract rank-related fields (name, elo, role_id) with validation
  pub fn extract_rank_data(row: &SqliteRow, guild_id: GI) -> Result<Option<Rank>> {
    let name: Option<String> = row.get("name");
    let rank_elo: Option<i64> = row.get("rank_elo");
    let role_id: Option<i64> = row.get("role_id");

    match (name, rank_elo, role_id) {
      (Some(name), Some(rank_elo), Some(role_id)) => Ok(Some(Rank { guild_id, role_id: RoleId::new(role_id as u64), name, elo: rank_elo as u16 })),
      _ => Ok(None), // Incomplete rank data
    }
  }

  /// Extract optional string field with fallback
  pub fn extract_opt_string(row: &SqliteRow, column: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(column).ok().flatten()
  }

  /// Extract i64 field with default value
  pub fn extract_i64_with_default(row: &SqliteRow, column: &str, default: i64) -> i64 {
    row.try_get::<i64, _>(column).unwrap_or(default)
  }

  /// Extract boolean from i64 field (0/1 to bool)
  pub fn extract_bool_from_i64(row: &SqliteRow, column: &str) -> bool {
    row.try_get::<i64, _>(column).unwrap_or(0) != 0
  }

  /// Extract channel ID with validation (rejects 0)
  pub fn extract_channel_id(row: &SqliteRow, column: &str) -> Result<u64> {
    let id: i64 = row.get(column);
    if id == 0 {
      anyhow::bail!("Invalid channel ID 0 for column {}", column);
    }
    Ok(id as u64)
  }

  /// Extract optional channel ID (allows 0 for None)
  pub fn extract_opt_channel_id(row: &SqliteRow, column: &str) -> Option<u64> {
    let id: i64 = row.get(column);
    if id == 0 {
      None
    } else {
      Some(id as u64)
    }
  }
}

/// Trait for database migration helpers
pub trait MigrationHelpers {
  /// Add a column if it doesn't exist
  async fn add_column_if_missing(&self, table: &str, column: &str, column_type: &str, default: &str) -> Result<()>;

  /// Add multiple columns from a list of (column, type, default) tuples
  async fn add_columns_if_missing(&self, table: &str, columns: &[(&str, &str, &str)]) -> Result<()>;
}

/// Macro for implementing common row mapping patterns
///
/// Usage:
/// ```rust
/// impl_row_mapping!(
///     MyStruct,
///     field1: i64 => "field1",
///     field2: String => "field2",
///     field3: Option<String> => "field3"
/// );
/// ```
#[macro_export]
macro_rules! impl_row_mapping {
    ($struct_name:ident, $($field:ident: $field_type:ty => $column:literal),+ $(,)?) => {
        impl $crate::db::helpers::RowMapper for $struct_name {
            fn from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Self>
            where
                Self: Sized,
            {
                Ok(Self {
                    $(
                        $field: row.get::<$field_type, _>($column),
                    )*
                })
            }
        }
    };
}

/// Macro for extracting multiple optional fields with proper error handling
#[macro_export]
macro_rules! extract_opt_fields {
    ($row:expr, $($field:ident: $type:ty => $column:literal),+ $(,)?) => {
        $(
            let $field: Option<$type> = $row.try_get::<Option<$type>, _>($column).ok().flatten();
        )*
    };
}

/// Macro for building INSERT queries with dynamic placeholders
#[macro_export]
macro_rules! build_batch_insert {
  ($table:literal, $columns:expr, $chunk_size:expr) => {{
    let placeholders: Vec<String> = (0..$chunk_size).map(|_| format!("({})", vec!["?"; $columns.len()].join(", "))).collect();
    format!("INSERT INTO {} ({}) VALUES {}", $table, $columns.join(", "), placeholders.join(", "))
  }};
}

/// Macro for adding multiple columns with the same pattern
#[macro_export]
macro_rules! add_columns {
    ($self:expr, $table:literal, $($column:literal: $type:literal => $default:literal),+ $(,)?) => {
        $(
            if !$self.check_column($table, $column).await? {
                sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {} DEFAULT {}", $table, $column, $type, $default))
                    .execute(&$self.pool)
                    .await?;
            }
        )*
    };
}
