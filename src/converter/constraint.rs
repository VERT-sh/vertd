use super::ConverterFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint<T> {
    Cap(T),
    Exact(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatConstraint {
    pub video_bitrate: Option<Constraint<u64>>,
    pub resolution: Option<Constraint<(u32, u32)>>,
    pub fps: Option<Constraint<u32>>,
    pub audio_bitrate: Option<Constraint<u64>>,
    pub audio_sample_rate: Option<Constraint<u32>>,
    pub audio_channels: Option<Constraint<u32>>,
    pub dynamic_audio_block_size: bool,
    pub extra_args: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedConstraint {
    pub bitrate: u64,
    pub fps: u32,
    pub requires_video_encoding: bool,
    pub requires_audio_encoding: bool,
    pub args: Vec<String>,
}

impl FormatConstraint {
    pub fn for_format(format: ConverterFormat) -> Option<Self> {
        match format {
            ConverterFormat::AMV => Some(Self {
                video_bitrate: Some(Constraint::Cap(384_000)),
                resolution: Some(Constraint::Cap((320, 240))),
                fps: Some(Constraint::Exact(30)),
                audio_bitrate: Some(Constraint::Cap(32_000)),
                audio_sample_rate: Some(Constraint::Exact(22_050)),
                audio_channels: Some(Constraint::Exact(1)),
                dynamic_audio_block_size: true,
                extra_args: &[],
            }),
            ConverterFormat::ThreeGP | ConverterFormat::ThreeG2 => Some(Self {
                video_bitrate: Some(Constraint::Cap(512_000)),
                resolution: Some(Constraint::Cap((320, 240))),
                fps: Some(Constraint::Exact(30)),
                audio_bitrate: Some(Constraint::Cap(16_000)),
                audio_sample_rate: Some(Constraint::Exact(22_050)),
                audio_channels: Some(Constraint::Exact(2)),
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::SWF => Some(Self {
                video_bitrate: Some(Constraint::Cap(800_000)),
                resolution: Some(Constraint::Cap((640, 480))),
                fps: Some(Constraint::Exact(30)),
                audio_bitrate: Some(Constraint::Cap(128_000)),
                audio_sample_rate: Some(Constraint::Exact(22_050)),
                audio_channels: None,
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::ASF | ConverterFormat::WMV => Some(Self {
                video_bitrate: Some(Constraint::Cap(1_500_000)),
                resolution: Some(Constraint::Cap((640, 480))),
                fps: Some(Constraint::Exact(30)),
                audio_bitrate: Some(Constraint::Cap(128_000)),
                audio_sample_rate: Some(Constraint::Exact(44_100)),
                audio_channels: Some(Constraint::Exact(2)),
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::MPEG | ConverterFormat::MPG | ConverterFormat::VOB => Some(Self {
                video_bitrate: Some(Constraint::Cap(2_000_000)),
                resolution: Some(Constraint::Cap((720, 576))),
                fps: Some(Constraint::Exact(30)),
                audio_bitrate: Some(Constraint::Cap(192_000)),
                audio_sample_rate: Some(Constraint::Exact(48_000)),
                audio_channels: None,
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::MXF => Some(Self {
                video_bitrate: None,
                resolution: None,
                fps: None,
                audio_bitrate: None,
                audio_sample_rate: Some(Constraint::Exact(48_000)),
                audio_channels: None,
                dynamic_audio_block_size: false,
                extra_args: &[],
            }),
            ConverterFormat::GXF => Some(Self {
                video_bitrate: None,
                resolution: Some(Constraint::Exact((720, 576))), // PAL resolution (but could also be 720x480 for NTSC)
                fps: None,
                audio_bitrate: None,
                audio_sample_rate: Some(Constraint::Exact(48_000)),
                audio_channels: Some(Constraint::Exact(1)),
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
    ) -> AppliedConstraint {
        let original_fps = fps;
        let mut bitrate = bitrate;
        let mut fps = fps;
        let mut args = Vec::new();
        let mut requires_video_encoding = false;
        let mut requires_audio_encoding = false;

        if auto_video_bitrate {
            if let Some(Constraint::Cap(cap)) = self.video_bitrate {
                if bitrate > cap {
                    bitrate = cap;
                    requires_video_encoding = true;
                }
            } else if let Some(Constraint::Exact(required)) = self.video_bitrate {
                if bitrate != required {
                    bitrate = required;
                    requires_video_encoding = true;
                }
            }
        }

        if auto_fps {
            if let Some(constraint) = self.fps {
                let should_adjust = match constraint {
                    Constraint::Cap(target) => original_fps > target,
                    Constraint::Exact(_) => true,
                };
                if should_adjust {
                    let target = match constraint {
                        Constraint::Cap(target) | Constraint::Exact(target) => target,
                    };
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
            if let Some(Constraint::Cap(audio_bitrate) | Constraint::Exact(audio_bitrate)) =
                self.audio_bitrate
            {
                args.extend(["-b:a".to_string(), audio_bitrate.to_string()]);
                requires_audio_encoding = true;
            }
        }

        if auto_sample_rate {
            if let Some(Constraint::Cap(sample_rate) | Constraint::Exact(sample_rate)) =
                self.audio_sample_rate
            {
                args.extend(["-ar".to_string(), sample_rate.to_string()]);
                requires_audio_encoding = true;
            }
        }

        if let Some(audio_channels) = self.audio_channels {
            let audio_channels = match audio_channels {
                Constraint::Cap(channels) | Constraint::Exact(channels) => channels,
            };
            args.extend(["-ac".to_string(), audio_channels.to_string()]);
            requires_audio_encoding = true;
        }

        if self.dynamic_audio_block_size {
            let effective_fps = fps.max(1);
            let effective_sample_rate = self
                .audio_sample_rate
                .map(|constraint| match constraint {
                    Constraint::Cap(sample_rate) | Constraint::Exact(sample_rate) => sample_rate,
                })
                .unwrap_or(22_050);
            let block_size = (effective_sample_rate / effective_fps).max(1);

            args.extend(["-block_size".to_string(), block_size.to_string()]);
            requires_audio_encoding = true;
        }

        if !self.extra_args.is_empty() {
            args.extend(self.extra_args.iter().map(|arg| (*arg).to_string()));
            requires_audio_encoding = true;
        }

        AppliedConstraint {
            bitrate,
            fps,
            requires_video_encoding,
            requires_audio_encoding,
            args,
        }
    }

    fn scale_filter(&self, resolution: (u32, u32)) -> Option<String> {
        let (input_width, input_height) = resolution;
        match self.resolution? {
            Constraint::Cap((max_width, max_height)) => {
                if input_width <= max_width && input_height <= max_height {
                    return None;
                }

                Some(format!(
                    "scale='min({},iw)':'min({},ih)':force_original_aspect_ratio=decrease",
                    max_width, max_height
                ))
            }
            Constraint::Exact((width, height)) => {
                if input_width == width && input_height == height {
                    return None;
                }

                Some(format!("scale={}:{}", width, height))
            }
        }
    }
}
