use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use format::{Conversion, ConverterFormat};
use job::{Job, ProgressUpdate};
use log::error;
use log::info;
use serde::{Deserialize, Serialize};
use speed::ConversionSpeed;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::mpsc;

pub mod format;
pub mod gpu;
pub mod job;
pub mod speed;
pub mod codecs;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConversionSettings {
    pub vertd_speed: Option<u8>,
    pub metadata: bool,
    pub fps: Option<String>,
    pub resolution: Option<String>,
    pub video_bitrate: Option<String>,
    pub audio_bitrate: Option<String>,
    pub sample_rate: Option<String>,
}

pub struct Converter {
    pub conversion: Conversion,
    speed: ConversionSpeed,
    settings: ConversionSettings,
}

impl Converter {
    pub fn new(
        from: ConverterFormat,
        to: ConverterFormat,
        speed: ConversionSpeed,
        settings: ConversionSettings,
    ) -> Self {
        Self {
            conversion: Conversion::new(from, to),
            speed,
            settings,
        }
    }

    pub async fn convert(
        &self,
        job: &mut Job,
        gpu: &gpu::ConverterGPU,
        vaapi_device_path: Option<&str>,
    ) -> anyhow::Result<(mpsc::Receiver<ProgressUpdate>, tokio::process::Child)> {
        let (tx, rx) = mpsc::channel(1);
        let input_filename = format!("input/{}.{}", job.id, self.conversion.from.to_string());
        let output_filename = format!("output/{}.{}", job.id, self.conversion.to.to_string());

        // use custom bitrate from speed if provided, else detect from file
        let bitrate = if let ConversionSpeed::Bitrate(b) = self.speed {
            b as u64
        } else {
            job.bitrate().await?
        };

        let fps = job.fps().await?;
        let (width, height) = job.resolution().await?;

        let app_state = crate::state::APP_STATE.lock().await;
        let supported_accelerated_codecs = &app_state.supported_accelerated_codecs;
        let args = self
            .conversion
            .to_args(
                &self.speed,
                gpu,
                (width, height),
                bitrate,
                fps,
                supported_accelerated_codecs,
                job,
                &self.settings,
            )
            .await?;
        let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        let args = args.as_slice();
        let gpu_args = gpu.hwaccel_args(vaapi_device_path);
        let gpu_args_refs: Vec<&str> = gpu_args.iter().map(|s| s.as_str()).collect();

        let metadata_args: &[&str] = if self.settings.metadata {
            &["-map_metadata", "0", "-map_chapters", "0"][..]
        } else {
            &["-map_metadata", "-1", "-map_chapters", "-1"][..]
        };

        let command = &[
            &[
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-progress",
                "pipe:1",
            ][..],
            &gpu_args_refs[..],
            &["-i", &input_filename][..],
            args,
            &metadata_args[..],
            &[output_filename.as_str()][..],
        ]
        .concat();
        let command = command
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        // if video is more than 4k on nvenc, remove -hwaccel cuda to avoid "Video width 7680 not within range from 48 to 4096"
        // error from the h264_nvenc *decoder*, guh
        let command = if matches!(gpu, gpu::ConverterGPU::NVIDIA) {
            let (width, height) = job.resolution().await?;
            if width > 3840 || height > 2160 {
                command
                    .iter()
                    .filter(|s| *s != "-hwaccel" && *s != "cuda")
                    .cloned()
                    .collect()
            } else {
                command
            }
        } else {
            command
        };

        info!("running 'ffmpeg {}'", command.join(" "));

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
                error!("{}", line);
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

        Ok((rx, process))
    }
}
