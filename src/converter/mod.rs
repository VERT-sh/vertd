use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use format::{Conversion, ConverterFormat};
use job::{Job, LogLine, ProgressUpdate};
use log::{error, info, warn};
use sequence::SequenceKind;
use serde::{Deserialize, Serialize};
use speed::ConversionSpeed;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::mpsc;

pub mod codecs;
pub mod constraint;
pub mod decoders;
pub mod fallback;
pub mod format;
pub mod gpu;
pub mod job;
pub mod pixfmt;
pub mod sequence;
pub mod speed;

#[derive(Debug, PartialEq, Eq)]
enum FfmpegLevel {
    Warn,
    Info,
    Error,
}

fn ffmpeg_level(line: &str) -> FfmpegLevel {
    let level = line
        .split_once("] [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(level, _)| level.trim().to_lowercase())
        .unwrap_or_default();

    match level.as_str() {
        "warning" => FfmpegLevel::Warn,
        "info" | "verbose" | "debug" | "trace" => FfmpegLevel::Info,
        _ => FfmpegLevel::Error,
    }
}

fn log_ffmpeg_line(line: &str) {
    match ffmpeg_level(line) {
        FfmpegLevel::Warn => warn!("{}", line),
        FfmpegLevel::Info => info!("{}", line),
        FfmpegLevel::Error => error!("{}", line),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConversionSettings {
    pub vertd_speed: Option<u8>,
    pub metadata: bool,
    pub fps: Option<String>,
    pub resolution: Option<String>,
    pub video_codec: Option<String>,
    pub video_bitrate: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_bitrate: Option<String>,
    pub audio_channels: Option<String>,
    pub sample_rate: Option<String>,
    #[serde(rename = "imageSequenceFPS")]
    pub image_sequence_fps: Option<u8>,
    pub image_sequence_transparency: Option<bool>,
}

impl ConversionSettings {
    fn validate_number(value: &Option<String>, name: &str, maximum: u32) -> anyhow::Result<()> {
        Self::validate_number_range(value, name, 1, maximum)
    }

    fn validate_number_range(
        value: &Option<String>,
        name: &str,
        minimum: u32,
        maximum: u32,
    ) -> anyhow::Result<()> {
        let Some(value) = value.as_deref() else {
            return Ok(());
        };
        if value.is_empty() || value == "auto" {
            return Ok(());
        }
        let number = value
            .parse::<u32>()
            .map_err(|_| anyhow!("{name} must be a positive integer"))?;
        if number < minimum || number > maximum {
            return Err(anyhow!("{name} must be between {minimum} and {maximum}"));
        }
        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        Self::validate_number(&self.fps, "fps", 240)?;
        Self::validate_number(&self.video_bitrate, "videoBitrate (kbps)", 125_000)?;
        Self::validate_number(&self.audio_bitrate, "audioBitrate (kbps)", 1_536)?;
        Self::validate_number(&self.audio_channels, "audioChannels", 8)?;
        Self::validate_number_range(&self.sample_rate, "sampleRate (Hz)", 8_000, 384_000)?;

        if let Some(resolution) = self.resolution.as_deref() {
            if !resolution.is_empty() && resolution != "auto" {
                let (width, height) = resolution
                    .split_once('x')
                    .ok_or_else(|| anyhow!("resolution must use WIDTHxHEIGHT"))?;
                let width = width
                    .parse::<u32>()
                    .map_err(|_| anyhow!("resolution width must be a positive integer"))?;
                let height = height
                    .parse::<u32>()
                    .map_err(|_| anyhow!("resolution height must be a positive integer"))?;
                if width == 0
                    || height == 0
                    || width > 8192
                    || height > 8192
                    || u64::from(width) * u64::from(height) > 33_554_432
                {
                    return Err(anyhow!(
                        "resolution dimensions must be 1-8192 with at most 33,554,432 pixels"
                    ));
                }
            }
        }
        Ok(())
    }
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
    ) -> anyhow::Result<(
        mpsc::Receiver<ProgressUpdate>,
        tokio::process::Child,
        job::LogBuffer,
    )> {
        self.convert_with_overrides(
            job,
            gpu,
            vaapi_device_path,
            fallback::RetryOverrides::default(),
        )
        .await
    }

    pub async fn convert_with_overrides(
        &self,
        job: &mut Job,
        gpu: &gpu::ConverterGPU,
        vaapi_device_path: Option<&str>,
        overrides: fallback::RetryOverrides,
    ) -> anyhow::Result<(
        mpsc::Receiver<ProgressUpdate>,
        tokio::process::Child,
        job::LogBuffer,
    )> {
        self.settings.validate()?;
        let (tx, rx) = mpsc::channel(256);
        let log_buffer = job::new_log_buffer();
        let input_filename = job.input_path();
        let output_filename = format!("output/{}.{}", job.id, self.conversion.to);

        let settings_fps: Option<f64> = match self.settings.fps.as_deref() {
            Some(fps) if !fps.is_empty() && fps != "auto" => Some(
                fps.parse::<f64>()
                    .map_err(|e| anyhow!("invalid fps '{}': {}", fps, e))?,
            ),
            _ => None,
        };

        let frame_sequence_fps = (job.sequence_kind() == Some(SequenceKind::FrameSequence))
            .then(|| self.settings.image_sequence_fps.unwrap_or(15));

        let sequence_args: Vec<String> = match frame_sequence_fps {
            Some(framerate) => vec![
                "-f".to_string(),
                "concat".to_string(),
                "-safe".to_string(),
                "0".to_string(),
                "-r".to_string(),
                framerate.to_string(),
            ],
            None => Vec::new(),
        };
        let sequence_args_refs: Vec<&str> = sequence_args.iter().map(|s| s.as_str()).collect();

        // use custom bitrate from speed if provided, else detect from file
        let bitrate = if let Some(bitrate) = self.speed.custom_bitrate_bps() {
            bitrate
        } else {
            job.bitrate().await?
        };

        let fps = match frame_sequence_fps {
            Some(framerate) => framerate as u32,
            None => match settings_fps {
                Some(framerate) => framerate.round() as u32,
                None => job.fps().await?,
            },
        };
        let (width, height) = job.resolution().await?;

        let supported_accelerated_codecs = {
            let app_state = crate::state::APP_STATE.lock().await;
            app_state.supported_accelerated_codecs.clone()
        };
        let (args, video_encoder) = self
            .conversion
            .to_args(
                &self.speed,
                gpu,
                (width, height),
                bitrate,
                fps,
                &supported_accelerated_codecs,
                job,
                &self.settings,
                overrides,
            )
            .await?;
        let args = args.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        let args = args.as_slice();
        let gpu_args = gpu.hwaccel_args(vaapi_device_path, overrides.sw_decode, &video_encoder);
        let gpu_args_refs: Vec<&str> = gpu_args.iter().map(|s| s.as_str()).collect();

        let metadata_args: &[&str] = if self.settings.metadata {
            &["-map_metadata", "0", "-map_chapters", "0"][..]
        } else {
            &["-map_metadata", "-1", "-map_chapters", "-1"][..]
        };

        let input_args = self.conversion.from.input_args(overrides.fix_timestamps);

        let avoid_negative_args: &[&str] = if overrides.fix_timestamps {
            &["-avoid_negative_ts", "make_zero"][..]
        } else {
            &[][..]
        };

        let command = &[
            &[
                "-y",
                "-hide_banner",
                "-loglevel",
                "level+warning",
                "-progress",
                "pipe:1",
            ][..],
            &gpu_args_refs[..],
            input_args,
            avoid_negative_args,
            &sequence_args_refs[..],
            &["-i", &input_filename][..],
            args,
            metadata_args,
            self.conversion.to.output_format_args(),
            &[output_filename.as_str()][..],
        ]
        .concat();
        let command = command
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        info!("running 'ffmpeg {}'", command.join(" "));

        let mut process = Command::new("ffmpeg")
            .kill_on_drop(true)
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

        let stderr_logs = Arc::clone(&log_buffer);
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut current: Option<LogLine> = None;
            loop {
                let line = match lines.next_line().await {
                    Ok(Some(line)) => line.trim_end().to_string(),
                    Ok(None) => break, // stderr closed cleanly
                    Err(e) => {
                        error!("failed to read ffmpeg stderr: {}", e);
                        break;
                    }
                };

                if line.trim().is_empty() {
                    continue;
                }

                match &mut current {
                    Some(entry) if entry.message == line => entry.repeats += 1,
                    _ => {
                        if let Some(entry) = current.take() {
                            job::push_log(&stderr_logs, entry.message, entry.repeats).await;
                        }
                        log_ffmpeg_line(&line);
                        current = Some(LogLine {
                            message: line,
                            repeats: 1,
                        });
                    }
                }
            }

            if let Some(entry) = current {
                job::push_log(&stderr_logs, entry.message, entry.repeats).await;
            }
        });

        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to take stdout"))?;
        let reader = BufReader::new(stdout);

        tokio::spawn(async move {
            let mut lines = reader.lines();
            let mut last_frames = None;
            let mut last_fps_bucket = None;
            loop {
                let out = match lines.next_line().await {
                    Ok(Some(out)) => out,
                    Ok(None) => break, // stdout closed cleanly
                    Err(e) => {
                        error!("failed to read ffmpeg progress output: {}", e);
                        break;
                    }
                };

                let mut map = HashMap::new();
                for line in out.split('\n') {
                    if let Some((k, v)) = line.split_once('=') {
                        map.insert(k.trim(), v.trim());
                    }
                }

                if let Some(frame) = map.get("frame").and_then(|s| s.parse::<u64>().ok()) {
                    if last_frames != Some(frame) {
                        last_frames = Some(frame);
                        if tx.send(ProgressUpdate::Frame(frame)).await.is_err() {
                            break;
                        }
                    }
                }

                if let Some(fps) = map.get("fps").and_then(|s| s.parse::<f64>().ok()) {
                    let bucket = (fps * 10.0).round() as i64;
                    if last_fps_bucket != Some(bucket) {
                        last_fps_bucket = Some(bucket);
                        if tx.send(ProgressUpdate::FPS(fps)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok((rx, process, log_buffer))
    }
}
