use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use uuid::Uuid;

use crate::job::JobTrait;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[skip_serializing_none]
pub struct CompressionJob {
    pub id: Uuid,
    pub auth: String,
    pub target_size_mb: Option<u32>,
}

impl JobTrait for CompressionJob {
    fn auth(&self) -> &str {
        &self.auth
    }

    fn id(&self) -> Uuid {
        self.id
    }

    fn handle_ws(&self, session: actix_ws::Session, stream: actix_ws::AggregatedMessageStream) {
        todo!("implement handle_ws for CompressionJob")
    }
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
