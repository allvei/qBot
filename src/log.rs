use tracing::info;

pub fn log_queue_toggle(guild_name: &str, group_name: &str, tag: &str, queue_type: QueueToggleType, pool_size: Option<(usize, usize)>, sg_name: Option<&str>) {
    let (action, source) = match queue_type {
        QueueToggleType::BJ => ("joined", "button"),
        QueueToggleType::BL => ("left",   "button"),
        QueueToggleType::VJ => ("joined", "vc"),
        QueueToggleType::VL => ("left",   "vc"),
    };

    let sg_part = sg_name.map(|n| format!(" {n} queue")).unwrap_or_default();

    match pool_size {
        Some((current, quota)) => info!("[{}/{}] {} {}{} ({}) [{}/{}]", guild_name, group_name, tag, action, sg_part, source, current, quota),
        None                   => info!("[{}/{}] {} {}{} ({})",         guild_name, group_name, tag, action, sg_part, source),
    }
}

pub enum QueueToggleType {
    BJ, // Button Join
    BL, // Button Leave
    VJ, // Voice Join
    VL, // Voice Leave
}
