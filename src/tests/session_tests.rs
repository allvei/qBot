use crate::models::player::{Player, Rank};
use crate::models::session::{Session, SessionStatus};
use crate::models::common::Team;

#[test]
fn test_bch_team_balancing() {
    // Create a test session using the new method
    let mut session = Session::new(1, 123456789);
    
    // Create players with different ranks
    let mut players = vec![
        create_test_player(1, Some(Rank::Beginner), false),     // ELO: 10
        create_test_player(2, Some(Rank::Novice), false),       // ELO: 30
        create_test_player(3, Some(Rank::Apprentice), false),   // ELO: 40
        create_test_player(4, Some(Rank::Journeyman), false),   // ELO: 50
        create_test_player(5, Some(Rank::Master), false),       // ELO: 65
        create_test_player(6, Some(Rank::MasterElite), false),  // ELO: 90
        create_test_player(7, Some(Rank::Grandmaster), false),  // ELO: 95
        create_test_player(8, Some(Rank::Beginner), false),     // ELO: 10
    ];
    
    // Add players to session
    for player in &players {
        session.add_player(player).unwrap();
    }
    
    // Generate teams using BCH algorithm
    session.generate_teams().unwrap();
    
    // Verify teams are balanced
    let mut team_red = Vec::new();
    let mut team_blue = Vec::new();
    
    for player in &session.pool {
        match player.team {
            Some(Team::Red) => team_red.push(player),
            Some(Team::Blue) => team_blue.push(player),
            None => panic!("Player should be assigned to a team"),
        }
    }
    
    // Check team sizes
    assert_eq!(team_red.len(), 4, "Team Red should have 4 players");
    assert_eq!(team_blue.len(), 4, "Team Blue should have 4 players");
    
    // Calculate team ELOs
    let team_red_elo: u32 = team_red.iter()
        .map(|p| p.rank.unwrap_or(Rank::Beginner).elo())
        .sum();
    
    let team_blue_elo: u32 = team_blue.iter()
        .map(|p| p.rank.unwrap_or(Rank::Beginner).elo())
        .sum();
    
    // Check that teams are reasonably balanced
    let elo_difference = (team_red_elo as i32 - team_blue_elo as i32).abs();
    assert!(elo_difference <= 30, "Teams should be balanced within 30 ELO points, got difference of {}", elo_difference);
    
    // Test with buffered players
    let mut session2 = Session::new(2, 123456789);
    
    // Create players with different ranks, some buffered
    let mut players2 = vec![
        create_test_player(11, Some(Rank::Beginner), true),     // ELO: 10, buffered
        create_test_player(12, Some(Rank::Novice), false),      // ELO: 30
        create_test_player(13, Some(Rank::Apprentice), true),   // ELO: 40, buffered
        create_test_player(14, Some(Rank::Journeyman), false),  // ELO: 50
        create_test_player(15, Some(Rank::Master), false),      // ELO: 65
        create_test_player(16, Some(Rank::MasterElite), false), // ELO: 90
        create_test_player(17, Some(Rank::Grandmaster), false), // ELO: 95
        create_test_player(18, Some(Rank::Beginner), false),    // ELO: 10
    ];
    
    // Add players to session
    for player in &players2 {
        session2.add_player(player).unwrap();
    }
    
    // Generate teams using BCH algorithm
    session2.generate_teams().unwrap();
    
    // Verify buffered players are included
    let buffered_players: Vec<_> = session2.pool.iter()
        .filter(|p| p.buffered)
        .collect();
    
    assert_eq!(buffered_players.len(), 2, "Should have 2 buffered players");
    
    // All buffered players should be assigned to teams
    for player in buffered_players {
        assert!(player.team.is_some(), "Buffered player should be assigned to a team");
    }
}

// Helper function to create test players
fn create_test_player(id: u64, rank: Option<Rank>, buffered: bool) -> Player {
    let mut player = Player {
        discord_id: id,
        steam_id: Some(id + 100000),
        guild_id: 123456789,
        session: Vec::new(),
        session_id: None,
        group_id: Some(123456789),
        rank,
        role: None,
        buffered,
        team: None,
    };
    
    if buffered {
        player.set_buffer_status(true);
    }
    
    player
}
