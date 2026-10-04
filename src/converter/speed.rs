use serde::{Deserialize, Serialize};

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
    Bitrate(u32),
}

impl ConversionSpeed {
    pub fn custom_bitrate_bps(&self) -> Option<u64> {
        match self {
            Self::Bitrate(kbps) => Some(u64::from(*kbps) * 1_000),
            _ => None,
        }
    }

    pub fn to_args(&self, encoder: &str, to: &ConverterFormat, bitrate: u64) -> Vec<String> {
        if encoder == "copy" {
            return Vec::new();
        }

        let level = match self {
            Self::UltraFast => 0,
            Self::Fast => 1,
            Self::Medium => 2,
            Self::Slow => 3,
            Self::Slower => 4,
            Self::VerySlow => 5,
            Self::Bitrate(_) => 2,
        };

        let mut args = Vec::new();

        if !matches!(self, Self::Bitrate(_)) {
            let knob = match encoder {
                "libx264" | "libx265" => Some((
                    "-preset",
                    ["ultrafast", "fast", "medium", "slow", "slower", "veryslow"][level],
                )),
                "libsvtav1" => Some(("-preset", ["12", "10", "8", "6", "4", "2"][level])),
                "libvpx" | "libvpx-vp9" => {
                    Some(("-cpu-used", ["8", "6", "4", "2", "1", "0"][level]))
                }
                "libaom-av1" => Some(("-cpu-used", ["8", "6", "4", "2", "1", "0"][level])),
                "h264_nvenc" | "hevc_nvenc" | "av1_nvenc" => {
                    Some(("-preset", ["p1", "p2", "p4", "p5", "p6", "p7"][level]))
                }
                "h264_amf" | "hevc_amf" | "av1_amf" => Some((
                    "-quality",
                    [
                        "speed", "speed", "balanced", "balanced", "quality", "quality",
                    ][level],
                )),
                _ => None,
            };
            if let Some((flag, value)) = knob {
                args.extend([flag.to_string(), value.to_string()]);
            }
        }

        if *to == ConverterFormat::WEBP && matches!(encoder, "libwebp" | "libwebp_anim") {
            let lossless = matches!(self, Self::Slow | Self::Slower | Self::VerySlow);
            args.extend(["-lossless".to_string(), u8::from(lossless).to_string()]);
        }
        if *to != ConverterFormat::GIF {
            args.extend(["-b:v".to_string(), bitrate.to_string()]);
        }
        args
    }
}
