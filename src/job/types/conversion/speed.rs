use log::warn;
use serde::{Deserialize, Serialize};

use crate::job::gpu::JobGPU;

use super::format::ConverterFormat;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ConversionSpeed {
    UltraFast,
    Fast,
    Medium,
    Slow,
    Slower,
    VerySlow,
}

impl ConversionSpeed {
    pub fn to_bitrate_mul(&self) -> f64 {
        match self {
            ConversionSpeed::UltraFast => 0.88,
            ConversionSpeed::Fast => 0.94,
            ConversionSpeed::Medium => 1.0,
            ConversionSpeed::Slow => 1.06,
            ConversionSpeed::Slower => 1.12,
            ConversionSpeed::VerySlow => 1.18,
        }
    }

    pub fn to_args(&self, to: &ConverterFormat, gpu: &JobGPU, bitrate: u64) -> Vec<String> {
        let mut args = Vec::new();

        match to {
            ConverterFormat::MP4
            | ConverterFormat::MKV
            | ConverterFormat::MOV
            | ConverterFormat::MTS => {
                args.push("-preset".to_string());
                match gpu {
                    JobGPU::NVIDIA => match self {
                        // only "slow", "medium", and "fast" are supported
                        ConversionSpeed::VerySlow | ConversionSpeed::Slower => {
                            args.push("slow".to_string())
                        }
                        ConversionSpeed::Slow | ConversionSpeed::Medium => {
                            args.push("medium".to_string())
                        }
                        ConversionSpeed::Fast | ConversionSpeed::UltraFast => {
                            args.push("fast".to_string())
                        }
                    },

                    _ => match self {
                        ConversionSpeed::UltraFast => args.push("ultrafast".to_string()),
                        ConversionSpeed::Fast => args.push("fast".to_string()),
                        ConversionSpeed::Medium => args.push("medium".to_string()),
                        ConversionSpeed::Slow => args.push("slow".to_string()),
                        ConversionSpeed::Slower => args.push("slower".to_string()),
                        ConversionSpeed::VerySlow => args.push("veryslow".to_string()),
                    },
                }
            }

            ConverterFormat::GIF => {}

            ConverterFormat::WebM | ConverterFormat::AVI => {
                args.push("-speed".to_string());
                match self {
                    ConversionSpeed::UltraFast => args.push("4".to_string()),
                    ConversionSpeed::Fast => args.push("3".to_string()),
                    ConversionSpeed::Medium => args.push("2".to_string()),
                    ConversionSpeed::Slow => args.push("1".to_string()),
                    ConversionSpeed::Slower => args.push("0".to_string()),
                    ConversionSpeed::VerySlow => args.push("-1".to_string()),
                };
            }

            ConverterFormat::WMV => {
                warn!("wmv format does not support speed settings");
            }
        };

        if *to != ConverterFormat::GIF {
            args.push("-b:v".to_string());
            let bitrate = (bitrate as f64 * self.to_bitrate_mul()) as u64;
            args.push(bitrate.to_string());
        }

        args
    }
}
