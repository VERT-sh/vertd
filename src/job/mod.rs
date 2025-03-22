pub mod gpu;
pub mod types;

use std::sync::Arc;

use actix_ws::{AggregatedMessageStream, Session};
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use strum_macros::AsRefStr;
use tokio::sync::Notify;
use types::{CompressionJob as Compression, ConversionJob as Conversion};
use uuid::Uuid;

#[enum_dispatch]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, AsRefStr)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
#[skip_serializing_none]
pub enum Job {
    Conversion,
    Compression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobType {
    Conversion,
    Compression,
}

#[enum_dispatch(Job)]
pub trait JobTrait: Clone {
    fn id(&self) -> Uuid;
    fn auth(&self) -> &str;

    async fn handle_ws(
        &mut self,
        session: Session,
        stream: AggregatedMessageStream,
    ) -> anyhow::Result<()>;

    fn completed(&self) -> bool;
    fn output_path(&self) -> Option<String>;
}
