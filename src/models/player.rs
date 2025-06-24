use serde::{Deserialize, Serialize};

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

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

// helper macro to count identifiers
macro_rules! count_idents {
    () => (0);
    ($_head:ident $(, $tail:ident)*) => (1 + count_idents!($($tail),*));
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    Runner,
    Admin,
}

define_ranks! {
    Beginner    => {id: 1357839644363849749, name:"Beginner",    elo: 0},
    Newcomer    => {id: 1259886204870983772, name:"Newcomer",    elo: 0},
    Apprentice  => {id: 1259886204870983772, name:"Apprentice",  elo: 0},
    Journeyman  => {id: 1259886033076752394, name:"Journeyman",  elo: 0},
    Expert      => {id: 1334602752336203836, name:"Expert",      elo: 0},
    Master      => {id: 1259885952361435237, name:"Master",      elo: 0},
    MasterElite => {id: 1261417176364093611, name:"Master Elite", elo: 0},
    Grandmaster => {id: 1261447652638326967, name:"Grandmaster", elo: 0}
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
    pub steam_id64: u64,
    pub role:       Option<Role>,
    pub rank:       Option<Rank>,
}

impl Player {
    pub fn new(discord_id: u64) -> Player {
        Player {
            discord_id,
            steam_id64: 0,
            role:       None,
            rank:       None,
        }
    }
}