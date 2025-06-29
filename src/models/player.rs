

use serde::{Deserialize, Serialize};
use crate::config::*;

// helper macro to count identifiers
macro_rules! count_idents {
    () => (0);
    ($_head:ident $(, $tail:ident)*) => (1 + count_idents!($($tail),*));
}

macro_rules! define_ranks {
    (
        $(
            $(#[$meta:meta])*
            $Variant:ident => {
                id   : $role_id:expr,
                name : $name:expr,
                elo  : $elo:expr
            }
        ),* $(,)?
    ) => {
        // generate the ID constants
        $(
            pub const $Variant: u64 = $role_id;
        )*
        
        #[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
        pub enum Rank {
            $(
                $(#[$meta])*
                $Variant { role_id: u64, elo: u32 },
            )*
        }
        
        impl Rank {
            pub fn all() -> &'static [Rank] {
                static RANKS: [Rank; count_idents!($($Variant),*)] = [
                    $(
                        Rank::$Variant { role_id: $Variant, elo: $elo },
                    )*
                ];
                &RANKS
            }

            pub fn title(&self) -> &'static str {
                match self {
                    $(Rank::$Variant {..} => $name, )*
                }
            }

            pub fn role_id(&self) -> u64 {
                match self {
                    $(Rank::$Variant { role_id, .. } => *role_id, )*
                }
            }

            pub fn elo(&self) -> u32 {
                match self {
                    $(Rank::$Variant { elo, .. } => *elo, )*
                }
            }

            pub fn from_title(s: &str) -> Option<Rank> {
                Self::all().iter().cloned()
                    .find(|r| r.title().eq_ignore_ascii_case(s))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    Runner{id: u64},
    Admin {id: u64},
}

#[allow(non_snake_case, unreachable_patterns)]
impl Role {
    pub fn id(&self) -> u64 {
        match self {
          Runner => ID_RUNNER,
          Admin  => ID_ADMIN,
        }
    }
}

// #TODO: Add dupe ranks for burgers
define_ranks! {
    EU_BEGINNER    => { id: ID_EU_BEGINNER,      name: "Beginner",     elo:  10 },
    EU_NEWCOMER    => { id: ID_EU_NEWCOMER,      name: "Newcomer",     elo:  30 },
    EU_NOVICE      => { id: ID_EU_NOVICE,        name: "Novice",       elo:  40 },
    EU_APPRENTICE  => { id: ID_EU_APPRENTICE,    name: "Apprentice",   elo:  50 },
    EU_JOURNEYMAN  => { id: ID_EU_JOURNEYMAN,    name: "Journeyman",   elo:  65 },
    EU_MASTER      => { id: ID_EU_MASTER,        name: "Master",       elo:  85 },
    EU_MASTERELITE => { id: ID_EU_MASTER_ELITE,  name: "Master Elite", elo:  90 },
    EU_GRANDMASTER => { id: ID_EU_GRANDMASTER,   name: "Grandmaster",  elo:  95 },

    // NA_BEGINNER    => { id: ID_NA_BEGINNER,      name: "Beginner",     elo:  10 },
    // NA_NEWCOMER    => { id: ID_NA_NEWCOMER,      name: "Newcomer",     elo:  30 },
    // NA_NOVICE      => { id: ID_NA_NOVICE,        name: "Novice",       elo:  40 },
    // NA_APPRENTICE  => { id: ID_NA_APPRENTICE,    name: "Apprentice",   elo:  50 },
    // NA_JOURNEYMAN  => { id: ID_NA_JOURNEYMAN,    name: "Journeyman",   elo:  65 },
    // NA_MASTER      => { id: ID_NA_MASTER,        name: "Master",       elo:  85 },
    // NA_MASTERELITE => { id: ID_NA_MASTER_ELITE,  name: "Master Elite", elo:  90 },
    // NA_GRANDMASTER => { id: ID_NA_GRANDMASTER,   name: "Grandmaster",  elo:  95 },
}

/// User data structure representing a player in the system
/// 
/// * `discord_id` - Discord user ID
/// * `steam_id64` - Steam 64-bit ID
/// * `elo`        - User's Elo rating
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::missing_docs_in_private_items)]
pub struct Player {
    pub discord_id: u64,
    pub steam_id:   u64,
    pub rank:       Option<Rank>,
    pub role:       Option<Role>,
}

impl Player {
    pub fn new(discord_id: u64) -> Player {
        Player {
            discord_id,
            steam_id: 0,
            rank:       None,
            role:       None,
        }
    }

    pub fn set_rank(&mut self, title: &str) -> Result<(), String> {
        self.rank = Some(Rank::from_title(title)
            .ok_or_else(|| format!("No such rank: {}", title))?);
        Ok(())
    }
}
