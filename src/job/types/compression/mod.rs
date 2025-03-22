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
    pub completed: bool,
    pub from: String,
}

impl JobTrait for CompressionJob {
    fn auth(&self) -> &str {
        &self.auth
    }

    fn id(&self) -> Uuid {
        self.id
    }

    async fn handle_ws(
        &mut self,
        _session: actix_ws::Session,
        _stream: actix_ws::AggregatedMessageStream,
    ) -> anyhow::Result<()> {
        todo!("implement handle_ws for CompressionJob")
    }

    fn completed(&self) -> bool {
        self.completed
    }

    fn output_path(&self) -> Option<String> {
        Some(format!("output/{}.{}", self.id, self.from))
    }
}

impl CompressionJob {
    pub fn new(auth_token: String, from: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            auth: auth_token,
            target_size_mb: None,
            completed: false,
            from,
        }
    }
}
