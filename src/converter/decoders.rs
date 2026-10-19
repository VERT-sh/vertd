use std::collections::HashSet;
use std::process::Stdio;

use log::{info, warn};
use tokio::process::Command;
use tokio::sync::OnceCell;

// audio codecs ffmpeg can decode, `None` if the probe failed
static DECODABLE_AUDIO_CODECS: OnceCell<Option<HashSet<String>>> = OnceCell::const_new();

// codecs ffmpeg has a decoder for, or `None` if the probe failed
pub async fn decodable_audio_codecs() -> Option<&'static HashSet<String>> {
    DECODABLE_AUDIO_CODECS
        .get_or_init(|| async {
            match probe_decodable_audio_codecs().await {
                Ok(codecs) => {
                    info!("found {} audio codecs with a decoder", codecs.len());
                    Some(codecs)
                }
                Err(e) => {
                    warn!(
                        "failed to list ffmpeg decoders, assuming every audio stream can be decoded: {e}"
                    );
                    None
                }
            }
        })
        .await
        .as_ref()
}

async fn probe_decodable_audio_codecs() -> anyhow::Result<HashSet<String>> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-codecs"])
        .stdin(Stdio::null())
        .output()
        .await?;

    anyhow::ensure!(
        output.status.success(),
        "ffmpeg -codecs exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );

    Ok(parse_decodable_audio_codecs(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

// " D.AI.S apple_apac    Apple Positional Audio Codec"
fn parse_decodable_audio_codecs(output: &str) -> HashSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let flags = fields.next()?.as_bytes();
            if flags.len() < 3 || flags[0] != b'D' || flags[2] != b'A' {
                return None;
            }
            fields.next().map(str::to_lowercase)
        })
        .collect()
}
