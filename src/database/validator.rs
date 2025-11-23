use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use super::migrations::DatabaseMigrations;
use super::repositories::GroupRepository;

/// Database validation and repair utility
pub struct DatabaseValidator {
    pool:       SqlitePool,
    migrations: DatabaseMigrations,
    group_repo: GroupRepository,
}

impl DatabaseValidator {
    pub fn new(pool: &SqlitePool) -> Self {
        Self {
            pool:       pool.clone(),
            migrations: DatabaseMigrations::new(pool),
            group_repo: GroupRepository::new(pool.clone()),
        }
    }

    /// Run comprehensive database validation
    pub async fn validate_all(&self) -> Result<ValidationReport> {
        info!("Starting database validation");

        let mut report = ValidationReport::new();

        // Schema validation
        match self.migrations.validate_schema().await {
            Ok(_) => {
                report.schema_valid = true;
                info!("Schema validation passed");
            },
            Err(e) => {
                report.schema_valid = false;
                report.errors.push(format!("Schema validation failed: {e}"));
                error!("Schema validation failed: {e}");
            }
        }

        // Data integrity validation
        self.validate_data_integrity(&mut report).await?;

        // Configuration validation
        self.validate_configurations(&mut report).await?;

        info!("Database validation completed");
        Ok(report)
    }

    /// Validate data integrity
    async fn validate_data_integrity(&self, report: &mut ValidationReport) -> Result<()> {
        info!("Validating data integrity");
        self.check_orphaned_records(report).await?;
        self.check_invalid_discord_ids(report).await?;
        self.check_duplicate_records(report).await?;

        Ok(())
    }

    /// Check for orphaned records
    async fn check_orphaned_records(&self, report: &mut ValidationReport) -> Result<()> {
        // Check for config entries without corresponding groups
        let orphaned_configs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM config c
             WHERE NOT EXISTS (SELECT 1 FROM groups g WHERE g.guild_id = c.guild)"
        )
        .fetch_one(&self.pool)
        .await?;

        if orphaned_configs > 0 {
            report.warnings.push(format!("{orphaned_configs} config entries have no corresponding groups"));
            warn!("Found {} orphaned config entries", orphaned_configs);
        }

        Ok(())
    }

    /// Check for invalid Discord IDs (placeholder values)
    async fn check_invalid_discord_ids(&self, report: &mut ValidationReport) -> Result<()> {
        let placeholder_groups: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM groups
             WHERE dashboard = 1 OR chat = 1 OR queue = 1 OR red = 1 OR blu = 1"
        )
        .fetch_one(&self.pool)
        .await?;

        if placeholder_groups > 0 {
            report.warnings.push(format!("{placeholder_groups} groups have placeholder Discord IDs"));
            warn!("Found {} groups with placeholder Discord IDs", placeholder_groups);
        }

        Ok(())
    }

    /// Check for duplicate records
    async fn check_duplicate_records(&self, report: &mut ValidationReport) -> Result<()> {
        // Check for duplicate users
        let duplicate_users: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) - COUNT(DISTINCT discord_id) FROM users"
        )
        .fetch_one(&self.pool)
        .await?;

        if duplicate_users > 0 {
            report.errors.push(format!("{duplicate_users} duplicate user records found"));
            error!("Found {} duplicate user records", duplicate_users);
        }

        Ok(())
    }

    /// Validate configurations
    async fn validate_configurations(&self, report: &mut ValidationReport) -> Result<()> {
        info!("Validating configurations");

        // Get all guilds from groups table
        let guild_ids: Vec<i64> = sqlx::query_scalar("SELECT DISTINCT guild_id FROM groups")
            .fetch_all(&self.pool)
            .await?;

        for guild_id in guild_ids {
            let guild_id_u64 = guild_id as u64;

            // Check if guild has at least one properly configured group
            match self.group_repo.get_groups_for_guild(guild_id_u64).await {
                Ok(groups) => {
                    if groups.is_empty() {
                        report.warnings.push(format!("Guild {guild_id} has no groups configured"));
                    } else {
                        report.guild_groups.insert(guild_id_u64, groups.len());
                        info!("Guild {guild_id} has {} group(s) configured", groups.len());
                    }
                },
                Err(e) => {
                    report.errors.push(format!("Failed to validate guild {guild_id}: {e}"));
                }
            }
        }

        Ok(())
    }

    /// Attempt to repair common database issues
    pub async fn repair_database(&self) -> Result<RepairReport> {
        info!("Starting database repair");

        let mut report = RepairReport::new();

        // Remove duplicate users
        let removed_duplicates = self.remove_duplicate_users().await?;
        if removed_duplicates > 0 {
            report.actions.push(format!("Removed {removed_duplicates} duplicate user records"));
        }

        // Update placeholder Discord IDs (this requires manual intervention)
        let placeholder_count = self.count_placeholder_ids().await?;
        if placeholder_count > 0 {
            report.manual_actions.push(format!(
                "Found {placeholder_count} groups with placeholder Discord IDs. These require manual configuration.",
            ));
        }

        info!("Database repair completed");
        Ok(report)
    }

    /// Remove duplicate users
    async fn remove_duplicate_users(&self) -> Result<i64> {
        let result = sqlx::query(
            "DELETE FROM users WHERE id NOT IN (
                SELECT MIN(id) FROM users GROUP BY discord_id
            )"
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Count groups with placeholder IDs
    async fn count_placeholder_ids(&self) -> Result<i64> {
        let count = sqlx::query_scalar(
            "SELECT COUNT(*) FROM groups
             WHERE dashboard = 1 OR chat = 1 OR queue = 1 OR red = 1 OR blu = 1"
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

    /// Create a default group for a guild
    pub async fn create_default_group(&self, guild_id: u64) -> Result<()> {
        self.migrations.ensure_default_group(guild_id).await
    }
}

/// Database validation report
#[derive(Debug)]
pub struct ValidationReport {
    pub schema_valid: bool,
    pub errors:       Vec<String>,
    pub warnings:     Vec<String>,
    pub guild_groups: std::collections::HashMap<u64, usize>,
}

impl ValidationReport {
    fn new() -> Self {
        Self {
            schema_valid: false,
            errors:       Vec::new(),
            warnings:     Vec::new(),
            guild_groups: std::collections::HashMap::new(),
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.schema_valid && self.errors.is_empty()
    }

    pub fn print_summary(&self) {
        info!("=== Database Validation Report ===");
        info!("Schema Valid: {}",       self.schema_valid);
        info!("Errors: {}",             self.errors.len());
        info!("Warnings: {}",           self.warnings.len());
        info!("Guilds with Groups: {}", self.guild_groups.len());

        if !self.errors.is_empty() {
            error!("Errors found:");
            for error in &self.errors {
                error!("  - {}", error);
            }
        }

        if !self.warnings.is_empty() {
            warn!("Warnings:");
            for warning in &self.warnings {
                warn!("  - {}", warning);
            }
        }

        info!("Guild configurations:");
        for (guild_id, group_count) in &self.guild_groups {
            info!("  Guild {}: {} group(s)", guild_id, group_count);
        }
    }
}

/// Database repair report
#[derive(Debug)]
pub struct RepairReport {
    pub actions:        Vec<String>,
    pub manual_actions: Vec<String>,
}

impl RepairReport {
    fn new() -> Self {
        Self {
            actions: Vec::new(),
            manual_actions: Vec::new(),
        }
    }

    pub fn print_summary(&self) {
        info!("=== Database Repair Report ===");

        if !self.actions.is_empty() {
            info!("Automated repairs performed:");
            for action in &self.actions {
                info!("  ✓ {}", action);
            }
        }

        if !self.manual_actions.is_empty() {
            warn!("Manual actions required:");
            for action in &self.manual_actions {
                warn!("  ! {}", action);
            }
        }

        if self.actions.is_empty() && self.manual_actions.is_empty() {
            info!("No repairs needed");
        }
    }
}
