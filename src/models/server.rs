// CHECK ME
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub ip: String,
    pub name: String,
}
