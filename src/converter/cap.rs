use super::ConverterFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatCap {
    pub video_bitrate: Option<u64>,
    pub resolution: Option<(u32, u32)>,
    pub fps: Option<u32>,
    pub audio_bitrate: Option<u64>,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u32>,
    pub dynamic_audio_block_size: bool,
    pub extra_args: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCap {
    pub bitrate: u64,
    pub fps: u32,
    pub requires_video_encoding: bool,
    pub requires_audio_encoding: bool,
    pub args: Vec<String>,
}

impl FormatCap {
    pub fn for_format(format: ConverterFormat) -> Option<Self> {
        match format {
            ConverterFormat::AMV => Some(Self {
                video_bitrate: Some(384_000),
                resolution: Some((320, 240)),
                fps: Some(30),
                audio_bitrate: Some(32_000),
                audio_sample_rate: Some(22_050),
                audio_channels: Some(1),
                dynamic_audio_block_size: true,
                extra_args: &[],
            }),
            ConverterFormat::ThreeGP | ConverterFormat::ThreeG2 => Some(Self {
                video_bitrate: Some(512_000),
                resolution: Some((320, 240)),
                fps: Some(30),
                audio_bitrate: Some(16_000),
                audio_sample_rate: Some(22_050),
                audio_channels: Some(2),
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::SWF => Some(Self {
                video_bitrate: Some(800_000),
                resolution: Some((640, 480)),
                fps: Some(30),
                audio_bitrate: Some(128_000),
                audio_sample_rate: Some(22_050),
                audio_channels: None,
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::ASF | ConverterFormat::WMV => Some(Self {
                video_bitrate: Some(1_500_000),
                resolution: Some((640, 480)),
                fps: Some(30),
                audio_bitrate: Some(128_000),
                audio_sample_rate: Some(44_100),
                audio_channels: None,
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::MPEG | ConverterFormat::MPG | ConverterFormat::VOB => Some(Self {
                video_bitrate: Some(2_000_000),
                resolution: Some((720, 576)),
                fps: Some(30),
                audio_bitrate: Some(192_000),
                audio_sample_rate: Some(48_000),
                audio_channels: None,
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            _ => None,
        }
    }

    pub fn apply(
        &self,
        bitrate: u64,
        fps: u32,
        resolution: (u32, u32),
        auto_video_bitrate: bool,
        auto_fps: bool,
        auto_resolution: bool,
        auto_audio_bitrate: bool,
        auto_sample_rate: bool,
    ) -> AppliedCap {
        let original_fps = fps;
        let mut bitrate = bitrate;
        let mut fps = fps;
        let mut args = Vec::new();
        let mut requires_video_encoding = false;
        let mut requires_audio_encoding = false;

        if auto_video_bitrate {
            if let Some(cap) = self.video_bitrate {
                if bitrate > cap {
                    bitrate = cap;
                    requires_video_encoding = true;
                }
            }
        }

        if auto_fps {
            if let Some(target) = self.fps {
                if original_fps > target {
                    fps = target;
                    args.extend(["-r".to_string(), target.to_string()]);
                    requires_video_encoding = true;
                }
            }
        }

        if auto_resolution {
            if let Some(scale) = self.scale_filter(resolution) {
                args.extend(["-vf".to_string(), scale]);
                requires_video_encoding = true;
            }
        }

        if auto_audio_bitrate {
            if let Some(audio_bitrate) = self.audio_bitrate {
                args.extend(["-b:a".to_string(), audio_bitrate.to_string()]);
                requires_audio_encoding = true;
            }
        }

        if auto_sample_rate {
            if let Some(sample_rate) = self.audio_sample_rate {
                args.extend(["-ar".to_string(), sample_rate.to_string()]);
                requires_audio_encoding = true;
            }
        }

        if let Some(audio_channels) = self.audio_channels {
            args.extend(["-ac".to_string(), audio_channels.to_string()]);
            requires_audio_encoding = true;
        }

        if self.dynamic_audio_block_size {
            let effective_fps = fps.max(1);
            let effective_sample_rate = self.audio_sample_rate.unwrap_or(22_050);
            let block_size = (effective_sample_rate / effective_fps).max(1);

            args.extend(["-block_size".to_string(), block_size.to_string()]);
            requires_audio_encoding = true;
        }

        if !self.extra_args.is_empty() {
            args.extend(self.extra_args.iter().map(|arg| (*arg).to_string()));
            requires_audio_encoding = true;
        }

        AppliedCap {
            bitrate,
            fps,
            requires_video_encoding,
            requires_audio_encoding,
            args,
        }
    }

    fn scale_filter(&self, resolution: (u32, u32)) -> Option<String> {
        let (input_width, input_height) = resolution;
        let (max_width, max_height) = self.resolution?;

        if input_width <= max_width && input_height <= max_height {
            return None;
        }

        Some(format!(
            "scale='min({},iw)':'min({},ih)':force_original_aspect_ratio=decrease",
            max_width, max_height
        ))
    }
}
