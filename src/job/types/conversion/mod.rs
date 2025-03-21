mod format;
mod handler;
mod speed;

pub use format::*;
pub use handler::*;
pub use speed::*;

// #[derive(Debug, Serialize, Deserialize)]
// #[serde(tag = "type", content = "data", rename_all = "camelCase")]
// pub enum ProgressUpdate {
//     #[serde(rename = "frame", rename_all = "camelCase")]
//     Frame(u64),
//     #[serde(rename = "fps", rename_all = "camelCase")]
//     FPS(f64),
//     #[serde(rename = "error", rename_all = "camelCase")]
//     Error(String),
// }
