use tracing::{info};

pub fn log_queue_toggle(guild_name: &str, group_name: &str, tag: &str, queue_type: QueueToggleType) {
    match queue_type {
        QueueToggleType::BJ => info!("[{}/{}] {} joined", guild_name, group_name, tag),
        QueueToggleType::BL => info!("[{}/{}] {} left",   guild_name, group_name, tag),
        QueueToggleType::VJ => info!("[{}/{}] {} joined", guild_name, group_name, tag),
        QueueToggleType::VL => info!("[{}/{}] {} left",   guild_name, group_name, tag),
    }
}

pub enum QueueToggleType {
    BJ, // Button Join
    BL, // Button Leave
    VJ, // Voice Join
    VL, // Voice Leave
}
