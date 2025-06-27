use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelGroup {
    pub dashboard:     u64,
    pub chat:          u64,
    pub queue:         u64,
    pub team_channels: Vec<TeamChannel>
}

impl ChannelGroup {
    pub fn new(dashboard: u64, chat: u64, queue: u64, red: u64, blu: u64) -> Self {
        Self { dashboard, chat, queue, 
               team_channels: vec![
                   TeamChannel::new(red, TeamColor::Red), 
                   TeamChannel::new(blu, TeamColor::Blu) ] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamChannel {
    pub id: u64,
    pub team: TeamColor,
}

impl TeamChannel {
    pub fn new(id: u64, team: TeamColor) -> Self {
        Self { id, team }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TeamColor {
    Red,
    Blu,
}