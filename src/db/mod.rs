pub mod helpers;
pub mod migrations;
pub mod repo;
pub mod validator;

use anyhow::Result;
use serenity::all::{Context as Ctx, GuildId as GI, UserId as UI};
use sqlx::SqlitePool;
use tracing::info;

use crate::{
  models::{Category, FileManager, Player, QGuild},
  repo::GuildRepository,
};
use migrations::DatabaseMigrations;
use repo::{CategoryRepository, ConfigRepository, EloRepository, FatkidRepository, MatchRepo, PlayerRepository, RankRepository, TeamRepository, UserServerPrefsRepository};

/// Main database interface that orchestrates all repositories
#[derive(Clone)]
pub struct Database {
  pub pool: SqlitePool,
  pub guilds: GuildRepository,
  pub players: PlayerRepository,
  pub categories: CategoryRepository,
  pub config: ConfigRepository,
  pub elo: EloRepository,
  pub ranks: RankRepository,
  pub teams: TeamRepository,
  pub matches: MatchRepo,
  pub fatkids: FatkidRepository,
  pub user_server_prefs: UserServerPrefsRepository,
}

impl Database {
  /// Creates a new Database instance and initializes all repositories
  pub async fn new(database_url: &str) -> Result<Self> {
    // Get the database path
    let db_path_str = database_url.strip_prefix("sqlite:").unwrap_or(database_url);

    // Check if the database file exists, create it if it doesn't
    if !db_path_str.is_empty() && !db_path_str.contains(":memory:") {
      let db_path = FileManager::normalize_path(db_path_str);
      if !FileManager::file_exists(&db_path) {
        info!("Missing database file, creating: {}", db_path.display());
        FileManager::create_file(&db_path)?;
      }
    }

    // Initialize the database connection pool
    let pool = SqlitePool::connect(database_url).await?;

    // Enable foreign key constraints (must be done per connection)
    sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;

    // Run migrations
    let migrations = DatabaseMigrations::new(&pool);
    migrations.create_tables().await?;

    // Initialize repositories
    let users = PlayerRepository::new(pool.clone());
    let guilds = GuildRepository::new(pool.clone());
    let categories = CategoryRepository::new(pool.clone());
    let config = ConfigRepository::new(pool.clone());
    let elos = EloRepository::new(pool.clone());
    let ranks = RankRepository::new(pool.clone());
    let teams = TeamRepository::new(pool.clone());
    let matches = MatchRepo::new(&pool);
    let fatkids = FatkidRepository::new(pool.clone());
    let user_server_prefs = UserServerPrefsRepository::new(pool.clone());

    // Verify schemas after all repositories are created
    migrations.verify_schemas().await?;

    Ok(Self { pool, guilds, players: users, categories, config, elo: elos, ranks, teams, matches, fatkids, user_server_prefs })
  }

  /// Get the underlying connection pool for advanced operations
  pub fn pool(&self) -> &SqlitePool {
    &self.pool
  }

  // Backward compatibility methods - delegate to repositories

  /// Creates a new user in the database
  pub async fn new_player(&self, user_id: UI, _ctx: &Ctx) -> Result<Player> {
    self.players.check_user(user_id, Some(0)).await
  }

  /// Gets a Player by Discord ID
  pub async fn get_player(&self, user_id: UI, ctx: &Ctx) -> Result<Player> {
    self.players.get_with_tag(user_id, ctx).await
  }

  /// Gets a Player by Discord ID
  pub async fn get_player_with_nick(&self, user_id: UI, ctx: &Ctx, guild_id: Option<serenity::all::GuildId>) -> Result<Player> {
    self.players.get_with_nick(user_id, ctx, guild_id).await
  }

  /// Updates a user's Steam ID
  pub async fn set_player_steam_id(&self, user_id: &UI, steam_id: Option<u64>) -> Result<Player> {
    self.players.update_steam_id(user_id, steam_id).await
  }

  /// Creates a new category
  #[allow(clippy::too_many_arguments)]
  pub async fn new_category(&self, guild_id: GI, guild_name: &str, category: u64, dashboard: u64, chat: u64, queue: u64, dashboard_msg: u64, quota: u8) -> Result<Category> {
    let config =
      repo::category::CategoryConfig { channel_category_id: category, dashboard_channel_id: dashboard, chat_channel_id: chat, queue_vc_id: queue, ping_channel_id: 1, quota };
    self.categories.add_category(guild_id, guild_name, dashboard_msg, config).await
  }

  /// Updates a category
  #[allow(clippy::too_many_arguments)]
  pub async fn set_category(&self, guild_id: GI, category: u64, queue_id: u64, dashboard: u64, chat: u64, quota: u8) -> Result<Category> {
    let config =
      repo::category::CategoryConfig { channel_category_id: category, dashboard_channel_id: dashboard, chat_channel_id: chat, queue_vc_id: queue_id, ping_channel_id: 1, quota };
    self.categories.update_category(guild_id, config).await
  }

  /// Gets configuration for a guild
  pub async fn get_config(&self, guild_id: GI) -> Result<QGuild> {
    // For now, return a simple Guild with the guild_id
    // The actual configuration is handled through get_config_map
    Ok(QGuild::empty(guild_id, "Unknown".to_string()))
  }
}
