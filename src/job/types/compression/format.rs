use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};

use crate::job::gpu::JobGPU;

#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumString, Display, Deserialize, Serialize)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum CompressorFormat {
    MP4,
}

impl CompressorFormat {
    pub async fn codec(&self, gpu: &JobGPU) -> String {
        match self {
            CompressorFormat::MP4 => gpu.accelerated_or_default_codec(&["h264"], "libx264").await,
        }
    }
}
