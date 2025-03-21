pub mod gpu;
pub mod types;

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use types::{CompressionJob, ConversionJob};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
#[skip_serializing_none]
pub enum Job {
    Conversion(ConversionJob),
    Compression(CompressionJob),
}

impl Into<JobType> for Job {
    fn into(self) -> JobType {
        match self {
            Job::Conversion(_) => JobType::Conversion,
            Job::Compression(_) => JobType::Compression,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobType {
    Conversion,
    Compression,
}
