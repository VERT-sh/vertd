use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[skip_serializing_none]
pub struct CompressionJob {
    pub id: Uuid,
    pub auth: String,
    pub target_size_mb: Option<u32>,
}

impl CompressionJob {
    pub fn new(auth_token: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            auth: auth_token,
            target_size_mb: None,
        }
    }
}
