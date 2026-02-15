use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{error, info, warn};
use serenity::all::GuildId as GI;

use super::migrations::DatabaseMigrations;
use super::repositories::CategoryRepository;

/// Database validation and repair utility
pub struct DatabaseValidator {
    pool:       SqlitePool,
    migrations: DatabaseMigrations,
    category_repo: CategoryRepository,
}

impl DatabaseValidator {
    pub fn new(pool: &SqlitePool) -> Self {
        Self {
            pool:       pool.clone(),
            migrations: DatabaseMigrations::new(pool),
            category_repo: CategoryRepository::new(pool.clone()),
        }
    }

    /// Run comprehensive database validation
    pub async fn validate_all(&self) -> Result<ValidationReport> {
        info!("Starting database validation");

        let mut report = ValidationReport::new();

        // Schema validation
        match self.migrations.verify_schemas().await {
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
        self.check_invalid_user_ids(report).await?;
        self.check_duplicate_records(report).await?;

        Ok(())
    }

    /// Check for orphaned records
    async fn check_orphaned_records(&self, report: &mut ValidationReport) -> Result<()> {
        // Check for config entries without corresponding categories
        let orphaned_configs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM config c
                                                        WHERE NOT EXISTS (SELECT 1 FROM categories g WHERE g.guild_id = c.guild)")
        .fetch_one(&self.pool)
        .await?;

        if orphaned_configs > 0 {
            report.warnings.push(format!("{orphaned_configs} config entries have no corresponding categories"));
            warn!("Found {} orphaned config entries", orphaned_configs);
        }

        Ok(())
    }

    /// Check for invalid Discord IDs (placeholder values)
    async fn check_invalid_user_ids(&self, report: &mut ValidationReport) -> Result<()> {
        let placeholder_categories: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories
                                                          WHERE dashboard = 1 OR chat = 1 OR queue = 1 OR red = 1 OR blu = 1")
        .fetch_one(&self.pool)
        .await?;

        if placeholder_categories > 0 {
            report.warnings.push(format!("{placeholder_categories} categories have placeholder Discord IDs"));
            warn!("Found {} categories with placeholder Discord IDs", placeholder_categories);
        }

        Ok(())
    }

    /// Check for duplicate records
    async fn check_duplicate_records(&self, report: &mut ValidationReport) -> Result<()> {
        // Check for duplicate users
        let duplicate_users: i64 = sqlx::query_scalar("SELECT COUNT(*) - COUNT(DISTINCT user_id) FROM users")
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

        // Get all guilds from categories table
        let guild_ids: Vec<i64> = sqlx::query_scalar("SELECT DISTINCT guild_id FROM categories")
            .fetch_all(&self.pool)
            .await?;

        for guild_id in guild_ids {
            // Check if guild has at least one properly configured category
            match self.category_repo.get_categories_for_guild(GI::new(guild_id as u64)).await {
                Ok(categories) => {
                    if categories.is_empty() {
                        report.warnings.push(format!("Guild {guild_id} has no categories configured"));
                    } else {
                        report.guild_categories.insert(guild_id as u64, categories.len());
                        info!("Guild {guild_id} has {} category(s) configured", categories.len());
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
                "Found {placeholder_count} categories with placeholder Discord IDs. These require manual configuration.",
            ));
        }

        info!("Database repair completed");
        Ok(report)
    }

    /// Remove duplicate users
    async fn remove_duplicate_users(&self) -> Result<i64> {
        let result = sqlx::query("DELETE FROM users WHERE user_id NOT IN (SELECT MIN(user_id) FROM users CATEGORY BY user_id )")
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Count categories with placeholder IDs
    async fn count_placeholder_ids(&self) -> Result<i64> {
        let count = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE dashboard = 1 OR chat = 1 OR queue = 1 OR red = 1 OR blu = 1")
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

    /// Create a default category for a guild
    pub async fn create_default_category(&self, guild_id: GI) -> Result<()> {
        self.migrations.init_first_category(guild_id).await
    }
}

/// Database validation report
#[derive(Debug)]
pub struct ValidationReport {
    pub schema_valid: bool,
    pub errors:       Vec<String>,
    pub warnings:     Vec<String>,
    pub guild_categories: std::collections::HashMap<u64, usize>,
}

impl ValidationReport {
    fn new() -> Self {
        Self {
            schema_valid: false,
            errors:       Vec::new(),
            warnings:     Vec::new(),
            guild_categories: std::collections::HashMap::new(),
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.schema_valid && self.errors.is_empty()
    }

    pub fn print_summary(&self) {
        info!("=== Database Validation Report ===");
        info!("Schema valid: {}",       self.schema_valid);
        info!("Errors: {}",             self.errors.len());
        info!("Warnings: {}",           self.warnings.len());
        info!("Guilds with categories: {}", self.guild_categories.len());

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
        for (guild_id, category_count) in &self.guild_categories {
            info!("  Guild {}: {} category(s)", guild_id, category_count);
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
            actions:        Vec::new(),
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
