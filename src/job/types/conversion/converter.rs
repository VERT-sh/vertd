use std::{collections::HashMap, sync::Arc};

use super::{speed::ConversionSpeed, Conversion, ConversionJob, ConverterFormat};
use crate::job::gpu::{self, JobGPU};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::Command,
    sync::mpsc,
};

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

    pub async fn convert(
        &self,
        job: &mut ConversionJob,
    ) -> anyhow::Result<mpsc::Receiver<ProgressUpdate>> {
        let (tx, rx) = mpsc::channel(1);
        let input_filename = format!("input/{}.{}", job.id, self.conversion.from.to_string());
        let output_filename = format!("output/{}.{}", job.id, self.conversion.to.to_string());
        // let gpu = gpu::get_gpu().await;
        // let bitrate = job.bitrate().await?;
        // let fps = job.fps().await?;
        // the above but we run in parallel
        let (gpu, (bitrate, fps)) = tokio::try_join!(gpu::get_gpu(), job.bitrate_and_fps())?;
        let args = self
            .conversion
            .to_args(&self.speed, &gpu, bitrate, fps)
            .await?;
        let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        let args = args.as_slice();

        let gpu_args: &[&str] = match gpu {
            JobGPU::AMD => &["-hwaccel", "amf"],
            JobGPU::Intel => &["-hwaccel", "qsv"],
            JobGPU::NVIDIA => &["-hwaccel", "cuda"],
            JobGPU::Apple => &["-hwaccel", "videotoolbox"],
        };

        let command = &[
            &["-hide_banner", "-loglevel", "error", "-progress", "pipe:1"],
            gpu_args,
            &["-i", &input_filename],
            args,
            &[&output_filename],
        ]
        .concat();
        let command = command
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        log::info!("running 'ffmpeg {}'", command.join(" "));

        let mut process = Command::new("ffmpeg")
            .args(command)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("failed to spawn ffmpeg: {}", e))?;

        let stderr = process
            .stderr
            .take()
            .ok_or_else(|| anyhow!("failed to take stderr"))?;

        let tx_arc = Arc::new(tx);

        let tx = Arc::clone(&tx_arc);

        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::error!("{}", line);
                tx.send(ProgressUpdate::Error(line)).await.unwrap();
            }
        });

        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to take stdout"))?;
        let reader = BufReader::new(stdout);

        let tx = Arc::clone(&tx_arc);

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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum ProgressUpdate {
    #[serde(rename = "frame", rename_all = "camelCase")]
    Frame(u64),
    #[serde(rename = "fps", rename_all = "camelCase")]
    FPS(f64),
    #[serde(rename = "error", rename_all = "camelCase")]
    Error(String),
}
