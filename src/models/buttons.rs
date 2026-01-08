use serenity::all::CreateButton as CB;
use serenity::all::ButtonStyle as BS;

pub fn toggle(custom_id: &str, label: &str, bool: bool) -> CB {
    CB::new(custom_id)
        .label(if bool { format!("{label} enabled") } else { format!("{label} disabled") })
        .style(if bool { BS::Success } else { BS::Danger })
}

pub fn edit(custom_id: &str, label: &str) -> CB {
    CB::new(custom_id)
        .label(label)
        .style(BS::Primary)
}

pub fn option(custom_id: &str, label: &str) -> CB {
    CB::new(custom_id)
        .label(label)
        .style(BS::Secondary)
}

pub fn close(custom_id: &str) -> CB {
    CB::new(custom_id)
        .label("Close")
        .style(BS::Danger)
}

#[macro_export]
macro_rules! row {
    ([$($btn:expr),* $(,)?]) => {
        serenity::all::CreateActionRow::Buttons(vec![$($btn),*])
    };
}