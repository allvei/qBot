use pfpug_bot::Database;
use sqlx::Row;

async fn setup_test_db() -> Database {
    // Use in-memory SQLite database for tests
    let db_url = "sqlite::memory:";
    
    Database::new(db_url).await.expect("Failed to create test database")
}

#[tokio::test]
async fn test_user_creation() {
    let db = setup_test_db().await;
    
    // Test creating a new user
    let user = db.get_or_create_user("test_discord_id", "testuser").await.unwrap();
    assert_eq!(user.discord_id, "test_discord_id");
    assert_eq!(user.username, "testuser");
    
    // Test getting existing user
    let same_user = db.get_or_create_user("test_discord_id", "testuser").await.unwrap();
    assert_eq!(same_user.id, user.id);
    assert_eq!(same_user.discord_id, "test_discord_id");
}

#[tokio::test]
async fn test_queue_operations() {
    let db = setup_test_db().await;
    
    // Create test user
    let user = db.get_or_create_user("user123", "testuser").await.unwrap();
    
    // Create a test channel ID
    let test_channel_id = "123456789";

    // Test joining queue with channel_id
    let query = "INSERT INTO queue_sessions (user_id, channel_id) VALUES (?, ?)"; 
    sqlx::query(query)
        .bind(user.id)
        .bind(test_channel_id)
        .execute(db.get_connection())
        .await
        .unwrap();

    // Test queue count by doing a direct query
    let count_query = "SELECT COUNT(*) as count FROM queue_sessions WHERE channel_id = ?"; 
    let row = sqlx::query(count_query)
        .bind(test_channel_id)
        .fetch_one(db.get_connection())
        .await
        .unwrap();
    let count: i64 = row.get("count");
    assert_eq!(count, 1);
    
    // Test getting queue players with direct query
    let players_query = "SELECT qs.id, qs.user_id, qs.channel_id, qs.joined_at, u.discord_id 
                       FROM queue_sessions qs 
                       JOIN users u ON qs.user_id = u.id 
                       WHERE qs.channel_id = ?";
    let rows = sqlx::query(players_query)
        .bind(test_channel_id)
        .fetch_all(db.get_connection())
        .await
        .unwrap();
    
    assert_eq!(rows.len(), 1);
    let discord_id: String = rows[0].get("discord_id");
    assert_eq!(discord_id, "user123");
    
    // Test leaving queue with direct query
    let delete_query = "DELETE FROM queue_sessions WHERE user_id = ?";
    sqlx::query(delete_query)
        .bind(user.id)
        .execute(db.get_connection())
        .await
        .unwrap();
    
    // Verify user was removed from queue
    let count_query = "SELECT COUNT(*) as count FROM queue_sessions WHERE channel_id = ?";
    let row = sqlx::query(count_query)
        .bind(test_channel_id)
        .fetch_one(db.get_connection())
        .await
        .unwrap();
    let count_after: i64 = row.get("count");
    assert_eq!(count_after, 0);
}

#[tokio::test]
async fn test_session_operations() {
    let db = setup_test_db().await;
    
    // Create test users
    let mut red_users = Vec::new();
    let mut blu_users = Vec::new();
    
    for i in 0..4 {
        let red_user = db.get_or_create_user(&format!("red_user_{}", i), &format!("red{}", i)).await.unwrap();
        let blu_user = db.get_or_create_user(&format!("blu_user_{}", i), &format!("blu{}", i)).await.unwrap();
        red_users.push(red_user);
        blu_users.push(blu_user);
    }
    
    // Test creating session - pass slices, not vectors
    let session = db.create_session(red_users.as_slice(), blu_users.as_slice(), "server_a").await.unwrap();
    assert_eq!(session.status, "hot");
    assert_eq!(session.server_channel, "server_a");
    
    // Test accepting session - this changes status from "hot" to "push"
    db.accept_session(session.id).await.unwrap();
    
    let updated_session = db.get_session_by_uuid(&session.session_uuid).await.unwrap();
    assert_eq!(updated_session.status, "push");
    assert!(updated_session.confirmed_at.is_some());
    
    // Test ending session - this changes status from "push" to "idle"
    db.end_session(session.id).await.unwrap();
    
    let ended_session = db.get_session_by_uuid(&session.session_uuid).await.unwrap();
    assert_eq!(ended_session.status, "idle");
    assert!(ended_session.ended_at.is_some());
}

#[tokio::test]
async fn test_config_operations() {
    let db = setup_test_db().await;
    
    // Test setting config
    db.set_config("test_key", "test_value").await.unwrap();
    
    // Test getting config - just verify it doesn't error
    let _config = db.get_config().await.unwrap();
}
