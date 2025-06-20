use pfpug_bot::{Database, models::*};

async fn setup_test_db() -> Database {
    // Use in-memory SQLite database for tests
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
async fn test_shuffle_action_team_generation() {
    let db = setup_test_db().await;
    
    // Create 8 test users
    let _user_ids = create_test_users(&db, 8).await;
    
    // Get user objects directly
    let mut users = Vec::new();
    for i in 0..8 {
        // Use get_or_create_user but with consistent discord IDs
        let user = db.get_or_create_user(&format!("discord_{}", i), &format!("user{}", i)).await.unwrap();
        users.push(user);
    }
    
    // Then add users to queue
    for user in &users {
        db.join_queue(user.id, queue::QueueType::Default).await.unwrap();
    }
    
    // Verify queue has 8 players
    let queue_count = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    assert_eq!(queue_count, 8);
    
    // Create session with shuffled teams (simulating shuffle action)
    let red_team = &users[0..4];
    let blu_team = &users[4..8];
    
    let session = db.create_session(red_team, blu_team, "server_a").await.unwrap();
    
    // Verify session was created with correct status
    assert_eq!(session.status, "hot");
    assert_eq!(session.server_channel, "server_a");
    
    // Get session players to verify team assignment
    let session_players = db.get_session_players(session.id).await.unwrap();
    assert_eq!(session_players.len(), 8);
    
    // Verify we have both red and blu teams - session_players is Vec<(String, String)> where first is discord_id, second is team
    let red_count = session_players.iter().filter(|(_, team)| team == "RED").count();
    let blu_count = session_players.iter().filter(|(_, team)| team == "BLU").count();
    assert_eq!(red_count, 4);
    assert_eq!(blu_count, 4);
    
    // Verify queue is now empty (players moved to session)
    let queue_after = db.get_queue_count(queue::QueueType::Default).await.unwrap();
    
    // Debug: If there are players in queue, let's see who they are
    if queue_after > 0 {
        let queue_players = db.get_queue_idle(queue::QueueType::Default).await.unwrap();
        println!("Queue still has {} player(s):", queue_after);
        for (session, user) in &queue_players {
            println!("  User ID: {}, Queue Type: {}, Joined At: {}", 
                     user.id, session.queue_type, session.joined_at);
        }
    }
    
    assert_eq!(queue_after, 0);
}

#[tokio::test]
async fn test_accept_action() {
    let db = setup_test_db().await;
    
    // Create users and session
    let user_ids = create_test_users(&db, 8).await;
    let mut users = Vec::new();
    for user_id in &user_ids {
        let user = db.get_or_create_user(&format!("{}", user_id), &format!("{}", user_id)).await.unwrap();
        users.push(user);
    }
    
    let red_team = &users[0..4];
    let blu_team = &users[4..8];
    
    let session = db.create_session(red_team, blu_team, "server_a").await.unwrap();
    
    // Verify initial status
    assert_eq!(session.status, "hot");
    
    // Accept the session
    db.accept_session(session.id).await.unwrap();
    
    // Verify status changed to push
    let updated_session = db.get_session_by_uuid(&session.session_uuid).await.unwrap();
    assert_eq!(updated_session.status, "push");
    assert!(updated_session.confirmed_at.is_some());
}

#[tokio::test]
async fn test_accept_action_with_latest_hot_session() {
    let db = setup_test_db().await;
    
    // Create users and session
    let user_ids = create_test_users(&db, 8).await;
    let mut users = Vec::new();
    for user_id in &user_ids {
        let user = db.get_or_create_user(&format!("{}", user_id), &format!("{}", user_id)).await.unwrap();
        users.push(user);
    }
    
    let red_team = &users[0..4];
    let blu_team = &users[4..8];
    
    let _session = db.create_session(red_team, blu_team, "server_a").await.unwrap();
    
    // Get latest hot session and accept it
    let latest_session = db.get_latest_hot_session().await.unwrap();
    db.accept_session(latest_session.id).await.unwrap();
    
    // Verify status changed
    let updated_session = db.get_session_by_uuid(&latest_session.session_uuid).await.unwrap();
    assert_eq!(updated_session.status, "push");
}

#[tokio::test]
async fn test_end_action() {
    let db = setup_test_db().await;
    
    // Create users and session
    let user_ids = create_test_users(&db, 8).await;
    let mut users = Vec::new();
    for user_id in &user_ids {
        let user = db.get_or_create_user(&format!("{}", user_id), &format!("{}", user_id)).await.unwrap();
        users.push(user);
    }
    
    let red_team = &users[0..4];
    let blu_team = &users[4..8];
    
    let session = db.create_session(red_team, blu_team, "server_a").await.unwrap();
    
    // Accept session first
    db.accept_session(session.id).await.unwrap();
    
    // End the session
    db.end_session(session.id).await.unwrap();
    
    // Verify status changed to idle
    let ended_session = db.get_session_by_uuid(&session.session_uuid).await.unwrap();
    assert_eq!(ended_session.status, "idle");
    assert!(ended_session.ended_at.is_some());
}

#[tokio::test]
async fn test_end_action_with_latest_push_session() {
    let db = setup_test_db().await;
    
    // Create users and session
    let user_ids = create_test_users(&db, 8).await;
    let mut users = Vec::new();
    for user_id in &user_ids {
        let user = db.get_or_create_user(&format!("{}", user_id), &format!("{}", user_id)).await.unwrap();
        users.push(user);
    }
    
    let red_team = &users[0..4];
    let blu_team = &users[4..8];
    
    let session = db.create_session(red_team, blu_team, "server_a").await.unwrap();
    
    // Accept session to make it push status
    db.accept_session(session.id).await.unwrap();
    
    // Get latest push session and end it
    let latest_session = db.get_latest_push_session().await.unwrap();
    db.end_session(latest_session.id).await.unwrap();
    
    // Verify status changed to idle
    let ended_session = db.get_session_by_uuid(&latest_session.session_uuid).await.unwrap();
    assert_eq!(ended_session.status, "idle");
}

#[tokio::test]
async fn test_session_lifecycle_complete() {
    let db = setup_test_db().await;
    
    // Create users and add to queue
    let user_ids = create_test_users(&db, 8).await;
    for user_id in &user_ids {
        db.join_queue(*user_id, queue::QueueType::Default).await.unwrap();
    }
    
    // Get users for session creation
    let mut users = Vec::new();
    for user_id in &user_ids {
        let user = db.get_or_create_user(&format!("{}", user_id), &format!("{}", user_id)).await.unwrap();
        users.push(user);
    }
    
    let red_team = &users[0..4];
    let blu_team = &users[4..8];car
    
    // 1. Create session (shuffle action)
    let session = db.create_session(red_team, blu_team, "server_a").await.unwrap();
    assert_eq!(session.status, "hot");
    
    // 2. Accept session (accept action)
    db.accept_session(session.id).await.unwrap();
    
    let accepted_session = db.get_session_by_uuid(&session.session_uuid).await.unwrap();
    assert_eq!(accepted_session.status, "push");
    
    // 3. End session (end action)
    db.end_session(session.id).await.unwrap();
    
    let ended_session = db.get_session_by_uuid(&session.session_uuid).await.unwrap();
    assert_eq!(ended_session.status, "idle");
    assert!(ended_session.ended_at.is_some());
}
