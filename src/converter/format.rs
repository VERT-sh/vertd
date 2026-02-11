use crate::converter::job::Job;

use super::{gpu::ConverterGPU, speed::ConversionSpeed};
use log::{info, warn};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use strum_macros::{Display, EnumString};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, EnumString, Display)]
#[strum(serialize_all = "lowercase")]
pub enum ConverterFormat {
    MP4,
    WebM,
    GIF,
    APNG,
    WEBP,
    AVI,
    MKV,
    WMV,
    MOV,
    MTS,
    TS,
    M2TS,
    MPEG,
    MPG,
    FLV,
    F4V,
    VOB,
    M4V,
    #[strum(serialize = "3gp")]
    ThreeGP,
    #[strum(serialize = "3g2")]
    ThreeG2,
    MXF,
    OGV,
    RM,
    RMVB,
    H264,
    DIVX,
    SWF,
    AMV,
    ASF,
    NUT,
}

// referring to this, there's prob more? https://obsproject.com/kb/audio-video-formats-guide#containers
const CONTAINER_FORMATS: [ConverterFormat; 7] = [
    ConverterFormat::MP4,
    ConverterFormat::MKV,
    ConverterFormat::MOV,
    ConverterFormat::MTS,
    ConverterFormat::TS,
    ConverterFormat::M2TS,
    ConverterFormat::FLV,
];
static CONTAINER_SUPPORT: Lazy<HashMap<ConverterFormat, Vec<&'static str>>> = Lazy::new(|| {
    HashMap::from([
        (
            ConverterFormat::MP4,
            vec![
                "264", "hevc", "265", "av1", "prores", "alac", "flac", "opus", "pcm_",
            ],
        ),
        (
            ConverterFormat::MKV,
            vec![
                "264", "hevc", "265", "av1", "prores", "alac", "flac", "opus", "pcm_",
            ],
        ),
        (
            ConverterFormat::MOV,
            vec![
                "264", "hevc", "265", "av1", "prores", "alac", "flac", "opus", "pcm_",
            ],
        ),
        (ConverterFormat::MTS, vec!["264", "hevc", "265"]),
        (ConverterFormat::TS, vec!["264", "hevc", "265"]),
        (ConverterFormat::M2TS, vec!["264", "hevc", "265"]),
        (ConverterFormat::FLV, vec!["264"]),
    ])
});

impl ConverterFormat {
    pub fn conversion_into_args(
        &self,
        speed: &ConversionSpeed,
        gpu: &ConverterGPU,
        bitrate: u64,
    ) -> Vec<String> {
        speed.to_args(self, gpu, bitrate)
    }
}

pub struct Conversion {
    pub from: ConverterFormat,
    pub to: ConverterFormat,
}

impl Conversion {
    pub fn new(from: ConverterFormat, to: ConverterFormat) -> Self {
        Self { from, to }
    }

    async fn accelerated_or_default_codec(
        &self,
        gpu: &ConverterGPU,
        codecs: &[&str],
        default: &str,
        supported_accelerated_codecs: &Vec<String>,
    ) -> String {
        for codec in codecs {
            // try all codecs in order and use first supported, else fallback to default
            if supported_accelerated_codecs
                .iter()
                .any(|c| c.contains(codec))
            {
                return gpu
                    .get_accelerated_codec(codec)
                    .await
                    .unwrap_or_else(|_| default.to_string());
            }
        }

        warn!(
            "no supported accelerated codec found for {:?}, falling back to default {}",
            self.to, default
        );
        default.to_string()
    }

    // workarounds for NVENC for "weirder" videos
    // i only got a NVIDIA GPU so i don't know what other "workarounds" other encoders might need
    // -maya
    async fn nvenc_args(
        &self,
        gpu: &ConverterGPU,
        resolution: (u32, u32),
        fps: u32,
        supported_accelerated_codecs: &Vec<String>,
        job: &Job,
    ) -> anyhow::Result<Vec<String>> {
        let (width, height) = resolution;
        let is_4k = width == 3840 || height == 2160;
        let is_above_4k = width > 3840 || height > 2160;
        let pix_fmt = job.pix_fmt().await?;

        // choose codec
        // prefer original codec, force h265 if original is h264 and (10bit or 4k)
        let codecs = job.codecs().await?;
        let has_h265 = codecs.0.to_lowercase().contains("hevc");
        let has_h264 = codecs.0.to_lowercase().contains("h264");
        let is_10bit = pix_fmt.contains("10le") || pix_fmt.contains("10be");
        let (codec_order, default) = if has_h265 || (has_h264 && (is_10bit || is_4k || is_above_4k))
        {
            (&["hevc"][..], "libx265")
        } else if has_h264 {
            (&["h264"][..], "libx264")
        } else {
            (&["h264"][..], "libx264")
        };

        // do we still really need to check for codec support? this function is only called if gpu is nvidia
        let encoder = self
            .accelerated_or_default_codec(gpu, codec_order, default, supported_accelerated_codecs)
            .await;

        let mut args = vec!["-c:v".to_string(), encoder.clone()];

        // convert to 8 bit if 10 bit on h264_nvenc
        if is_10bit && encoder == "h264_nvenc" {
            args.extend(["-pix_fmt".to_string(), "yuv420p".to_string()]);
        }

        if fps > 240 {
            args.extend(["-r".to_string(), "240".to_string()]);
        }

        // scale to 160:-1 if width is less than 160
        if width < 160 {
            args.extend(["-vf".to_string(), "scale=160:-1".to_string()]);
        }

        Ok(args)
    }

    pub async fn to_args(
        &self,
        speed: &ConversionSpeed,
        gpu: &ConverterGPU,
        resolution: (u32, u32),
        bitrate: u64,
        fps: u32,
        supported_accelerated_codecs: &Vec<String>,
        job: &super::job::Job,
    ) -> anyhow::Result<Vec<String>> {
        let conversion_opts: Vec<String> = match self.to {
            ConverterFormat::MP4
            | ConverterFormat::MKV
            | ConverterFormat::MOV
            | ConverterFormat::MTS
            | ConverterFormat::TS
            | ConverterFormat::M2TS
            | ConverterFormat::FLV
            | ConverterFormat::F4V
            | ConverterFormat::M4V
            | ConverterFormat::ThreeGP
            | ConverterFormat::ThreeG2
            | ConverterFormat::H264 => {
                // remux if container format
                if CONTAINER_FORMATS.contains(&self.from) && CONTAINER_FORMATS.contains(&self.to) {
                    self.remux_args(gpu, supported_accelerated_codecs, job)
                        .await
                } else {
                    // else get args for re-encoding
                    if matches!(gpu, ConverterGPU::NVIDIA) {
                        self.nvenc_args(gpu, resolution, fps, supported_accelerated_codecs, job)
                            .await?
                    } else {
                        let encoder = self
                            .accelerated_or_default_codec(
                                gpu,
                                &["h264"][..],
                                "libx264",
                                supported_accelerated_codecs,
                            )
                            .await;
                        vec![
                            "-c:v".to_string(),
                            encoder,
                            "-c:a".to_string(),
                            "aac".to_string(),
                            "-strict".to_string(),
                            "experimental".to_string(),
                        ]
                    }
                }
            }

            ConverterFormat::GIF => {
                vec![
                    "-filter_complex".to_string(), 
                    format!(
                        "fps={},scale=800:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=64[p];[s1][p]paletteuse=dither=bayer",
                        fps.min(24)
                    )
                ]
            }

            ConverterFormat::APNG => {
                vec![
                    "-c:v".to_string(),
                    "apng".to_string(),
                ]
            }

            ConverterFormat::WEBP => {
                vec![
                    "-c:v".to_string(),
                    "libwebp".to_string(),
                    // lossless flag from speed.rs
                ]
            }

            // wmv2/3 doesn't actually have acceleration support on any gpu lmao
            // should prob just remove this since we have the supported_accelerated_codecs check, but maybe
            // we should just implement multiple retries in general with different settings/args for any sort of failure?
            ConverterFormat::WMV => {
                vec![
                    "-c:v".to_string(),
                    "wmv3".to_string(),
                    "-c:a".to_string(),
                    "wmav2".to_string(),
                ]
            }

            ConverterFormat::WebM => {
                let encoder = self
                    .accelerated_or_default_codec(
                        gpu,
                        &["av1", "vp9", "vp8"][..],
                        "libvpx",
                        supported_accelerated_codecs,
                    )
                    .await;
                vec![
                    "-c:v".to_string(),
                    encoder.to_string(),
                    "-c:a".to_string(),
                    "libvorbis".to_string(),
                ]
            }

            ConverterFormat::NUT | ConverterFormat::AVI => vec![
                "-c:v".to_string(),
                "mpeg4".to_string(),
                "-c:a".to_string(),
                "libmp3lame".to_string(),
            ],

            ConverterFormat::MPEG | ConverterFormat::MPG | ConverterFormat::VOB => {
                let encoder = self
                    .accelerated_or_default_codec(
                        gpu,
                        &["mpeg2"][..],
                        "mpeg2video",
                        supported_accelerated_codecs,
                    )
                    .await;
                vec![
                    "-c:v".to_string(),
                    encoder,
                    "-c:a".to_string(),
                    "mp2".to_string(),
                ]
            }

            // there is more formats that mxf supports (e.g. on cameras)
            ConverterFormat::MXF => {
                let encoder = self
                    .accelerated_or_default_codec(
                        gpu,
                        &["mpeg2"][..],
                        "mpeg2video",
                        supported_accelerated_codecs,
                    )
                    .await;
                vec![
                    "-c:v".to_string(),
                    encoder,
                    "-c:a".to_string(),
                    "pcm_s16le".to_string(),
                    "-strict".to_string(),
                    "unofficial".to_string(),
                ]
            }

            ConverterFormat::OGV => vec![
                "-c:v".to_string(),
                "libtheora".to_string(),
                "-c:a".to_string(),
                "libvorbis".to_string(),
            ],

            ConverterFormat::DIVX => vec![
                "-f".to_string(),
                "avi".to_string(),
                "-c:v".to_string(),
                "mpeg4".to_string(),
                "-c:a".to_string(),
                "libmp3lame".to_string(),
            ],

            ConverterFormat::SWF => vec![
                "-f".to_string(),
                "swf".to_string(),
                "-c:v".to_string(),
                "flv".to_string(),
                "-c:a".to_string(),
                "libmp3lame".to_string(),
                "-b:a".to_string(),
                "192k".to_string(),
            ],

            ConverterFormat::ASF => vec![
                "-c:v".to_string(),
                "msmpeg4v3".to_string(),
                "-c:a".to_string(),
                "wmav2".to_string(),
            ],

            ConverterFormat::AMV => vec![
                "-c:v".to_string(),
                "amv".to_string(),
                "-c:a".to_string(),
                "adpcm_ima_amv".to_string(),
                "-ac".to_string(),
                "1".to_string(),
                "-ar".to_string(),
                "22050".to_string(),
                "-r".to_string(),
                "25".to_string(),
                "-block_size".to_string(),
                "882".to_string(),
                "-strict".to_string(),
                "-1".to_string(),
            ],

            ConverterFormat::RM | ConverterFormat::RMVB => {
                warn!(
                    "encoding to {} is not supported, skipping job {}",
                    self.to, job.id
                );
                return Err(anyhow::anyhow!("encoding to {} is not supported", self.to));
            }
        };

        let conversion_opts = conversion_opts
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        let result = [
            conversion_opts,
            self.to.conversion_into_args(speed, gpu, bitrate),
        ]
        .concat();

        Ok(result)
    }

    async fn remux_args(
        &self,
        gpu: &ConverterGPU,
        supported_accelerated_codecs: &Vec<String>,
        job: &Job,
    ) -> Vec<String> {
        // referring to this, there's prob more? https://obsproject.com/kb/audio-video-formats-guide#containers
        // video codecs
        // h264 - all supported
        // hevc - all but flv
        // av1 - mp4 and mkv
        // prores - mov and mkv

        // audio codecs
        // aac - all
        // alac - mp4, mov, mkv
        // flac - mp4 and mkv
        // opus - all but flv and mov
        // pcm - mp4, mov, mkv

        let mut args = vec!["-c".to_string(), "copy".to_string()];
        let codecs = job
            .codecs()
            .await
            .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));
        let video_codec = codecs.0.to_lowercase();
        let audio_codec = codecs.1.to_lowercase();

        let supported_video_codecs = CONTAINER_SUPPORT
            .get(&self.to)
            .cloned()
            .unwrap_or_else(|| vec![]);
        let supported_audio_codecs = CONTAINER_SUPPORT
            .get(&self.to)
            .cloned()
            .unwrap_or_else(|| vec![]);

        if !supported_video_codecs
            .iter()
            .any(|c| video_codec.contains(c))
        {
            let encoder = self
                .accelerated_or_default_codec(
                    gpu,
                    &["h264"][..],
                    "libx264",
                    supported_accelerated_codecs,
                )
                .await;
            args.extend(["-c:v".to_string(), encoder]);
        }

        if audio_codec != "none"
            && !supported_audio_codecs
                .iter()
                .any(|c| audio_codec.contains(c))
        {
            args.extend(["-c:a".to_string(), "aac".to_string()]);
        }

        info!("performing remux for job {}", job.id);

        args
    }
}
