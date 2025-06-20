use pfpug_bot::{Database, models::*};

async fn setup_test_db() -> Database {
    // Use in-memory SQLite database for tests
    let db_url = "sqlite::memory:";
    
    Database::new(db_url).await.expect("Failed to create test database")
}

#[tokio::test]
async fn test_queue_join_action() {
    let db = setup_test_db().await;
    
    // Create test user
    let user = db.get_or_create_user("user123", "testuser").await.unwrap();
    
    // Test joining queue when empty
    let initial_count = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    assert_eq!(initial_count, 0);
    
    // Join queue
    db.join_queue(user.id, queue::QueueType::Default).await.unwrap();
    
    // Verify user is in queue
    let count_after_join = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    assert_eq!(count_after_join, 1);
    
    let queue_players = db.get_queue_idle(queue::QueueType::Default).await.unwrap();
    assert_eq!(queue_players.len(), 1);
    assert_eq!(queue_players[0].1.discord_id, "user123");
    
    // Test joining queue when already in queue (should not duplicate)
    let _result = db.join_queue(user.id, queue::QueueType::Default).await;
    // This should either succeed (idempotent) or fail gracefully
    let final_count = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    assert_eq!(final_count, 1); // Should still be 1, not 2
}

#[tokio::test]
async fn test_queue_leave_action() {
    let db = setup_test_db().await;
    
    // Create test user
    let user = db.get_or_create_user("user123", "testuser").await.unwrap();
    
    // Join queue first
    db.join_queue(user.id, queue::QueueType::Default).await.unwrap();
    let count_after_join = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    assert_eq!(count_after_join, 1);
    
    // Leave queue
    db.leave_queue_by_user_id(user.id).await.unwrap();
    
    // Verify user is removed from queue
    let count_after_leave = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    assert_eq!(count_after_leave, 0);
    
    let queue_players = db.get_queue_idle(queue::QueueType::Default).await.unwrap();
    assert_eq!(queue_players.len(), 0);
}

#[tokio::test]
async fn test_queue_status_action() {
    let db = setup_test_db().await;
    
    // Test empty queue status
    let empty_queue = db.get_queue_idle(queue::QueueType::Default).await.unwrap();
    assert_eq!(empty_queue.len(), 0);
    
    // Add multiple users to queue
    let user_names = vec!["user1", "user2", "user3"];
    let mut user_ids = Vec::new();
    
    for (i, name) in user_names.iter().enumerate() {
        let user = db.get_or_create_user(&format!("discord_{}", i), name).await.unwrap();
        db.join_queue(user.id, queue::QueueType::Default).await.unwrap();
        user_ids.push(user.id);
    }
    
    // Test queue status with players
    let queue_with_players = db.get_queue_idle(queue::QueueType::Default).await.unwrap();
    assert_eq!(queue_with_players.len(), 3);
    
    // Verify all users are in queue
    let usernames: Vec<String> = queue_with_players.iter()
        .map(|(_, user)| user.username.clone())
        .collect();
    assert!(usernames.contains(&"user1".to_string()));
    assert!(usernames.contains(&"user2".to_string()));
    assert!(usernames.contains(&"user3".to_string()));
}

#[tokio::test]
async fn test_queue_quota_reached() {
    let db = setup_test_db().await;
    
    // Add 8 users to reach quota
    let mut user_ids = Vec::new();
    for i in 0..8 {
        let user = db.get_or_create_user(&format!("discord_{}", i), &format!("user{}", i)).await.unwrap();
        db.join_queue(user.id, queue::QueueType::Default).await.unwrap();
        user_ids.push(user.id);
    }
    
    // Verify queue has 8 players
    let queue_count = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    assert_eq!(queue_count, 8);
    
    let queue_players = db.get_queue_idle(queue::QueueType::Default).await.unwrap();
    assert_eq!(queue_players.len(), 8);
    
    // This simulates when quota is reached and session should be created
    // In actual implementation, this would trigger team generation
    assert!(queue_count >= 8); // Quota reached condition
}

#[tokio::test]
async fn test_multiple_queue_types() {
    let db = setup_test_db().await;
    
    // Create users for different queue types
    let user1 = db.get_or_create_user("user1", "newcomer_user").await.unwrap();
    let user2 = db.get_or_create_user("user2", "journey_user").await.unwrap();
    let user3 = db.get_or_create_user("user3", "default_user").await.unwrap();
    
    // Add users to different queue types
    db.join_queue(user1.id, queue::QueueType::Newcomer).await.unwrap();
    db.join_queue(user2.id, queue::QueueType::Journey).await.unwrap();
    db.join_queue(user3.id, queue::QueueType::Default).await.unwrap();
    
    // Verify each queue type has correct count
    let newcomer_count = db.get_queue_count(queue::QueueType::Newcomer).await.unwrap();
    let journey_count = db.get_queue_count(queue::QueueType::Journey).await.unwrap();
    let default_count = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    
    assert_eq!(newcomer_count, 1);
    assert_eq!(journey_count, 1);
    assert_eq!(default_count, 1);
    
    // Verify users are in correct queues
    let newcomer_queue = db.get_queue_idle(queue::QueueType::Newcomer).await.unwrap();
    let journey_queue = db.get_queue_idle(queue::QueueType::Journey).await.unwrap();
    let default_queue = db.get_queue_idle(queue::QueueType::Default).await.unwrap();
    
    assert_eq!(newcomer_queue[0].1.username, "newcomer_user");
    assert_eq!(journey_queue[0].1.username, "journey_user");
    assert_eq!(default_queue[0].1.username, "default_user");
}
