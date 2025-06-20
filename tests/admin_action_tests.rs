use pfpug_bot::Database;

async fn setup_test_db() -> Database {
    // Use in-memory SQLite for tests
    let db_url = "sqlite::memory:";
    
    Database::new(db_url).await.expect("Failed to create test database")
}

async fn create_test_users(db: &Database, count: usize) -> Vec<i64> {
    let mut user_ids = Vec::new();
    for i in 0..count {
        let user = db.get_or_create_user(&format!("discord_{}", i), &format!("user{}", i)).await.unwrap();
        user_ids.push(user.id);
    }
    user_ids
}

#[tokio::test]
async fn test_config_view_action() {
    let db = setup_test_db().await;
    
    // Get initial configuration
    let config = db.get_config().await.unwrap();
    
    // In migrations, default config values are defined but are empty strings
    // Check that all expected configuration keys are present
    assert_eq!(config.queue_channel_id, "");
    assert_eq!(config.log_channel_id, "");
    assert_eq!(config.queue_size, 8); // This one has a default value of 8
    assert_eq!(config.confirmation_timeout, 120); // This one has a default value of 120
}

#[tokio::test]
async fn test_config_update_actions() {
    let db = setup_test_db().await;
    
    // Test updating queue channel
    db.set_config("queue_channel_id", "123456789").await.unwrap();
    let config_after_queue = db.get_config().await.unwrap();
    assert_eq!(config_after_queue.queue_channel_id, "123456789");
    
    // Test updating log channel
    db.set_config("log_channel_id", "987654321").await.unwrap();
    let config_after_log = db.get_config().await.unwrap();
    assert_eq!(config_after_log.log_channel_id, "987654321");
    
    // Test updating server channel configuration
    db.set_config("red_a_channel_id", "red_a_123").await.unwrap();
    db.set_config("blu_a_channel_id", "blue_a_123").await.unwrap();
    db.set_config("red_b_channel_id", "red_b_456").await.unwrap();
    db.set_config("blu_b_channel_id", "blue_b_456").await.unwrap();
    
    let final_config = db.get_config().await.unwrap();
    assert_eq!(final_config.red_a_channel_id, "red_a_123");
    assert_eq!(final_config.blu_a_channel_id, "blue_a_123");
    assert_eq!(final_config.red_b_channel_id, "red_b_456");
    assert_eq!(final_config.blu_b_channel_id, "blue_b_456");
}

#[tokio::test]
async fn test_config_invalid_key_handling() {
    let db = setup_test_db().await;
    
    // Test handling of invalid config key
    let result = db.set_config("invalid_key", "some_value").await;
    
    // Should succeed but not affect actual config - config system should handle unknown keys gracefully
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_spot_exception_add_benched_player() {
    let db = setup_test_db().await;
    
    // Create test users with consistent IDs
    let mut users = Vec::new();
    for i in 0..8 {
        let user = db.get_or_create_user(&format!("discord_{}", i), &format!("user{}", i)).await.unwrap();
        users.push(user);
    }
    
    let red_team = &users[0..4];
    let blu_team = &users[4..8];
    
    let session = db.create_session(red_team, blu_team, "A").await.unwrap();
    
    // Create an additional user to be benched
    let benched_user = db.get_or_create_user("benched_discord", "benched_user").await.unwrap();
    
    // Add benched player to session (simulating /spot_exception command)
    db.add_player_to_session(session.id, benched_user.id, "red", true).await.unwrap();
    
    // Verify the benched player was added
    let players = db.get_session_players(session.id).await.unwrap();
    assert_eq!(players.len(), 9); // 8 original + 1 benched
    
    // Find the benched player - players is Vec<(String, String)> where first is discord_id, second is team
    let benched_found = players.iter().any(|(discord_id, team)| *discord_id == "benched_discord" && *team == "RED");
    assert!(benched_found);
    
    // Verify we still have the expected team distribution
    let red_count = players.iter().filter(|(_, team)| *team == "RED").count();
    let blu_count = players.iter().filter(|(_, team)| *team == "BLU").count();
    assert_eq!(red_count, 5); // 4 original + 1 benched
    assert_eq!(blu_count, 4); // 4 original
}

#[tokio::test]
async fn test_spot_exception_multiple_benched_players() {
    let db = setup_test_db().await;
    
    // Create test users with consistent IDs
    let mut users = Vec::new();
    for i in 0..8 {
        let user = db.get_or_create_user(&format!("discord_{}", i), &format!("user{}", i)).await.unwrap();
        users.push(user);
    }
    
    let red_team = &users[0..4];
    let blu_team = &users[4..8];
    
    let session = db.create_session(red_team, blu_team, "B").await.unwrap();
    
    // Create two additional users to be benched
    let benched_user1 = db.get_or_create_user("benched_discord_1", "benched_user1").await.unwrap();
    let benched_user2 = db.get_or_create_user("benched_discord_2", "benched_user2").await.unwrap();
    
    // Add both benched players to session
    db.add_player_to_session(session.id, benched_user1.id, "red", true).await.unwrap();
    db.add_player_to_session(session.id, benched_user2.id, "blu", true).await.unwrap();
    
    // Verify both benched players were added
    let players = db.get_session_players(session.id).await.unwrap();
    assert_eq!(players.len(), 10); // 8 original + 2 benched
    
    // Check that both benched players are in the session
    let benched1_found = players.iter().any(|(discord_id, team)| *discord_id == "benched_discord_1" && *team == "RED");
    let benched2_found = players.iter().any(|(discord_id, team)| *discord_id == "benched_discord_2" && *team == "BLU");
    assert!(benched1_found);
    assert!(benched2_found);
}

#[tokio::test]
async fn test_admin_session_management() {
    let db = setup_test_db().await;
    
    // Create multiple sessions to test admin overview
    let user_ids = create_test_users(&db, 16).await;
    
    // Get user objects for session creation
    let mut users = Vec::new();
    for user_id in &user_ids {
        let user = db.get_or_create_user(&format!("discord_{}", user_id), &format!("user{}", user_id)).await.unwrap();
        users.push(user);
    }
    
    // Create first session
    let red_team1 = &users[0..4];
    let blu_team1 = &users[4..8];
    let session1 = db.create_session(red_team1, blu_team1, "A").await.unwrap();
    
    // Create second session
    let red_team2 = &users[8..12];
    let blu_team2 = &users[12..16];
    let session2 = db.create_session(red_team2, blu_team2, "B").await.unwrap();
    
    // Accept first session
    db.accept_session(session1.id).await.unwrap();
    
    // Admin should be able to view hot sessions
    let hot_session = db.get_latest_hot_session().await.unwrap();
    assert_eq!(hot_session.session_uuid, session2.session_uuid);
    
    // Admin should be able to view push sessions
    let push_session = db.get_latest_push_session().await.unwrap();
    assert_eq!(push_session.session_uuid, session1.session_uuid);
    
    // End both sessions
    db.end_session(session1.id).await.unwrap();
    db.end_session(session2.id).await.unwrap();
    
    // Verify both sessions ended
    let ended_session1 = db.get_session_by_uuid(&session1.session_uuid).await.unwrap();
    let ended_session2 = db.get_session_by_uuid(&session2.session_uuid).await.unwrap();
    assert_eq!(ended_session1.status, "idle");
    assert_eq!(ended_session2.status, "idle");
    assert!(ended_session1.ended_at.is_some());
    assert!(ended_session2.ended_at.is_some());
}

#[tokio::test]
async fn test_admin_queue_management() {
    // In this test, we'll simulate managing players in different channel-based queues
    let db = setup_test_db().await;
    
    // Create users for the queue
    let user_ids = create_test_users(&db, 8).await;
    
    // Add database helpers to add users to channel-specific queues
    // These simulate the different queue types but work with our schema
    async fn add_user_to_channel_queue(db: &Database, user_id: i64, channel_id: &str) -> anyhow::Result<()> {
        // Create a custom query to add a user to a queue with a specific channel_id
        // This works with our schema that uses channel_id but not queue_type
        let query = "INSERT INTO queue_sessions (user_id, channel_id) VALUES (?, ?)";
        let _ = sqlx::query(query)
            .bind(user_id)
            .bind(channel_id)
            .execute(db.get_connection())
            .await?;
        Ok(())
    }
    
    async fn get_channel_queue_count(db: &Database, channel_id: &str) -> anyhow::Result<i64> {
        // Query to count users in a specific channel queue
        let query = "SELECT COUNT(*) FROM queue_sessions WHERE channel_id = ?";
        let count = sqlx::query_scalar::<_, i64>(query)
            .bind(channel_id)
            .fetch_one(db.get_connection())
            .await?;
        Ok(count)
    }
    
    // Add users to different channel queues
    for i in 0..4 {
        add_user_to_channel_queue(&db, user_ids[i], "default_channel").await.unwrap();
    }
    
    for i in 4..8 {
        add_user_to_channel_queue(&db, user_ids[i], "newcomer_channel").await.unwrap();
    }
    
    // Verify users are in the queue
    let default_count = get_channel_queue_count(&db, "default_channel").await.unwrap();
    let newcomer_count = get_channel_queue_count(&db, "newcomer_channel").await.unwrap();
    
    assert_eq!(default_count, 4);
    assert_eq!(newcomer_count, 4);
    
    // Admin can remove users from queues
    db.leave_queue_by_user_id(user_ids[0]).await.unwrap();
    db.leave_queue_by_user_id(user_ids[4]).await.unwrap();
    
    // Verify queue counts after removal
    let default_after = get_channel_queue_count(&db, "default_channel").await.unwrap();
    let newcomer_after = get_channel_queue_count(&db, "newcomer_channel").await.unwrap();
    
    assert_eq!(default_after, 3);
    assert_eq!(newcomer_after, 3);
}
