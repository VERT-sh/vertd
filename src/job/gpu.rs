use anyhow::anyhow;
use log::warn;
use std::fmt::{self, Display, Formatter};
use tokio::process::Command;
use wgpu::Instance;

pub enum JobGPU {
    AMD,
    Intel,
    NVIDIA,
    Apple,
}

impl JobGPU {
    pub async fn get_accelerated_codec(&self, codec: &str) -> anyhow::Result<String> {
        let priority = self.encoder_priority();
        let encoders = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .await
            .map_err(|e| anyhow!("failed to get encoder support: {}", e))?;
        let encoders = String::from_utf8(encoders.stdout)?;
        for encoder in priority {
            let encoder = format!("{}_{}", codec, encoder);
            if encoders.contains(&encoder) {
                return Ok(encoder);
            }
        }

        Err(anyhow!("no supported encoder found for {}", codec))
    }

    pub fn encoder_priority(&self) -> Vec<&str> {
        match self {
            JobGPU::AMD => vec!["amf"],
            JobGPU::Intel => vec!["qsv"],
            JobGPU::NVIDIA => vec!["nvenc"],
            JobGPU::Apple => vec!["videotoolbox"],
        }
    }

    pub async fn accelerated_or_default_codec(&self, codecs: &[&str], default: &str) -> String {
        for codec in codecs {
            if let Ok(encoder) = self.get_accelerated_codec(codec).await {
                return encoder;
            }
        }
        default.to_string()
    }
}

impl Display for JobGPU {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            JobGPU::AMD => write!(f, "AMD"),
            JobGPU::Intel => write!(f, "Intel"),
            JobGPU::NVIDIA => write!(f, "NVIDIA"),
            JobGPU::Apple => write!(f, "Apple"),
        }
    }
}

async fn is_docker() -> bool {
    let dockerenv = tokio::fs::metadata("/.dockerenv").await.is_ok();
    let cgroup = tokio::fs::metadata("/proc/1/cgroup").await.is_ok();
    dockerenv || cgroup
}

pub async fn get_gpu() -> anyhow::Result<JobGPU> {
    let instance = Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .ok_or_else(|| anyhow!("no compatible adapter found"))?;

    let info = adapter.get_info();
    if info.name.contains("Apple") {
        return Ok(JobGPU::Apple);
    }
    match info.vendor {
        0x10DE => Ok(JobGPU::NVIDIA),
        0x1022 => Ok(JobGPU::AMD),
        0x8086 => Ok(JobGPU::Intel), // fun fact: intel's vendor id is 0x8086, presumably in reference to the intel 8086 processor
        0x106B | 0x0 => Ok(JobGPU::Apple),
        0x10005 if is_docker().await => {
            warn!("are you in a docker container? assuming NVIDIA, please open a PR and fix this if you're not.");
            Ok(JobGPU::NVIDIA)
        }
        _ => Err(anyhow!("unknown GPU vendor: 0x{:X}", info.vendor)),
    }
}
