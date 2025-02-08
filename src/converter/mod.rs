use std::collections::HashMap;

use anyhow::anyhow;
use format::{Conversion, ConverterFormat};
use job::{Job, ProgressUpdate};
use speed::ConversionSpeed;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::mpsc;

pub mod format;
pub mod job;
pub mod speed;

pub struct Converter {
    pub conversion: Conversion,
    speed: ConversionSpeed,
}

impl Converter {
    pub fn new(from: ConverterFormat, to: ConverterFormat, speed: ConversionSpeed) -> Self {
        Self {
            conversion: Conversion::new(from, to),
            speed,
        }
    }

    pub async fn convert(&self, job: &Job) -> anyhow::Result<mpsc::Receiver<ProgressUpdate>> {
        let (tx, rx) = mpsc::channel(1);
        let input_filename = format!("input/{}.{}", job.id, self.conversion.from.to_str());
        let output_filename = format!("output/{}.{}", job.id, self.conversion.to.to_str());
        let args = self.conversion.to_args(&self.speed);
        let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        let args = args.as_slice();

        let command = &[
            &[
                "-hide_banner",
                "-loglevel",
                "error",
                "-progress",
                "pipe:1",
                "-hwaccel",
                "cuda",
                "-i",
                &input_filename,
            ],
            args,
            &[&output_filename],
        ]
        .concat();
        let command = command
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        let mut process = Command::new("ffmpeg")
            .args(command)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("failed to spawn ffmpeg: {}", e))?;

        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to take stdout"))?;
        let reader = BufReader::new(stdout);

        tokio::spawn(async move {
            let mut lines = reader.lines();
            while let Ok(Some(out)) = lines.next_line().await {
                let mut map = HashMap::new();
                for line in out.split("\n") {
                    if let Some((k, v)) = line.split_once("=") {
                        map.insert(k.trim(), v.trim());
                    }
                }

                let mut reports = Vec::new();

                if let Some(frame) = map.get("frame").and_then(|s| s.parse().ok()) {
                    reports.push(ProgressUpdate::Frame(frame));
                }

                if let Some(fps) = map.get("fps").and_then(|s| s.parse().ok()) {
                    reports.push(ProgressUpdate::FPS(fps));
                }

                for report in reports {
                    if tx.send(report).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}
