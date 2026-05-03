use crate::converter::job::Job;

use super::{
    cap::FormatCap, codecs, gpu::ConverterGPU, speed::ConversionSpeed, ConversionSettings,
};
use log::{info, warn};
use strum_macros::{Display, EnumIter, EnumString};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
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
        supported_accelerated_codecs: &[String],
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

    fn default_encoder_for_codec(codec: &str) -> String {
        match codec {
            "h264" => "libx264".to_string(),
            "hevc" => "libx265".to_string(),
            "av1" => "libsvtav1".to_string(),
            "vp8" => "libvpx".to_string(),
            "vp9" => "libvpx-vp9".to_string(),
            "prores" => "prores_ks".to_string(),
            "webp" => "libwebp".to_string(),
            "flv1" => "flv".to_string(),
            "mp3" => "libmp3lame".to_string(),
            other => other.to_string(),
        }
    }

    fn is_auto(setting: &Option<String>) -> bool {
        match setting.as_deref() {
            None => true,
            Some("auto") => true,
            Some(_) => false,
        }
    }

    fn custom_value(setting: &Option<String>) -> Option<&str> {
        match setting.as_deref() {
            Some("auto") | None => None,
            Some(value) => Some(value),
        }
    }

    fn has_explicit_setting(settings: &[&Option<String>]) -> bool {
        settings.iter().any(|setting| !Self::is_auto(setting))
    }

    fn insert_codec_arg(args: &mut Vec<String>, flag: &str, codec: String) {
        if let Some(index) = args.iter().position(|arg| arg == flag) {
            if let Some(value) = args.get_mut(index + 1) {
                *value = codec;
                return;
            }
            args.push(codec);
            return;
        }

        args.extend([flag.to_string(), codec]);
    }

    async fn preferred_video_encoder(
        &self,
        gpu: &ConverterGPU,
        supported_accelerated_codecs: &[String],
    ) -> Option<String> {
        let (video_codecs, _) = codecs::codec_support_for(self.to)?;
        let preferred_video_codec = *video_codecs.first()?;
        let default_encoder = Self::default_encoder_for_codec(preferred_video_codec);

        Some(
            self.accelerated_or_default_codec(
                gpu,
                video_codecs,
                &default_encoder,
                supported_accelerated_codecs,
            )
            .await,
        )
    }

    fn preferred_audio_encoder(&self) -> Option<String> {
        let (_, audio_codecs) = codecs::codec_support_for(self.to)?;
        let preferred_audio_codec = *audio_codecs.first()?;
        Some(Self::default_encoder_for_codec(preferred_audio_codec))
    }

    // workarounds for NVENC for "weirder" videos
    // i only got a NVIDIA GPU so i don't know what other "workarounds" other encoders might need
    // -maya
    async fn nvenc_args(
        &self,
        gpu: &ConverterGPU,
        resolution: (u32, u32),
        fps: u32,
        supported_accelerated_codecs: &[String],
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

        args.extend(["-strict".to_string(), "experimental".to_string()]);

        Ok(args)
    }

    pub async fn to_args(
        &self,
        speed: &ConversionSpeed,
        gpu: &ConverterGPU,
        resolution: (u32, u32),
        bitrate: u64,
        fps: u32,
        supported_accelerated_codecs: &[String],
        job: &super::job::Job,
        settings: &ConversionSettings,
    ) -> anyhow::Result<Vec<String>> {
        let cap = FormatCap::for_format(self.to);

        let auto_video_bitrate = Self::is_auto(&settings.video_bitrate);
        let auto_fps = Self::is_auto(&settings.fps);
        let auto_resolution = Self::is_auto(&settings.resolution);
        let auto_audio_bitrate = Self::is_auto(&settings.audio_bitrate);
        let auto_video_codec = Self::is_auto(&settings.video_codec);
        let auto_audio_codec = Self::is_auto(&settings.audio_codec);
        let auto_sample_rate = Self::is_auto(&settings.sample_rate);
        let gif_width = Self::custom_value(&settings.resolution)
            .and_then(|custom_resolution| {
                custom_resolution
                    .split_once('x')
                    .and_then(|(width, _)| width.parse::<u32>().ok())
            })
            .unwrap_or(resolution.0);

        let applied_cap = cap.as_ref().map(|cap| {
            cap.apply(
                bitrate,
                fps,
                resolution,
                auto_video_bitrate,
                auto_fps,
                auto_resolution,
                auto_audio_bitrate,
                auto_sample_rate,
            )
        });

        info!(
            "applied cap for job {} (to {}): {:?}",
            job.id, self.to, applied_cap
        );

        let effective_bitrate = applied_cap
            .as_ref()
            .map_or(bitrate, |applied| applied.bitrate);
        let cap_args = applied_cap
            .as_ref()
            .map_or_else(Vec::new, |applied| applied.args.clone());

        let requires_video_encoding = Self::has_explicit_setting(&[
            &settings.fps,
            &settings.resolution,
            &settings.video_codec,
            &settings.video_bitrate,
        ]) || applied_cap
            .as_ref()
            .is_some_and(|applied| applied.requires_video_encoding);

        let requires_audio_encoding = Self::has_explicit_setting(&[
            &settings.audio_bitrate,
            &settings.audio_codec,
            &settings.sample_rate,
        ]) || applied_cap
            .as_ref()
            .is_some_and(|applied| applied.requires_audio_encoding);

        let input_codecs = job
            .codecs()
            .await
            .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));
        let input_video_codec = input_codecs.0.to_lowercase();
        let input_audio_codec = input_codecs.1.to_lowercase();

        let supports_remux = codecs::codec_support_for(self.from).is_some()
            && codecs::codec_support_for(self.to).is_some();

        let can_remux_video = supports_remux
            && !requires_video_encoding
            && codecs::support_video_codec(self.from, &input_video_codec)
            && codecs::support_video_codec(self.to, &input_video_codec);

        let can_remux_audio = supports_remux
            && !requires_audio_encoding
            && input_audio_codec != "none"
            && codecs::support_audio_codec(self.from, &input_audio_codec)
            && codecs::support_audio_codec(self.to, &input_audio_codec);

        let mut remux_streams = Vec::new();
        if can_remux_video {
            remux_streams.push("video".to_string());
        }
        if can_remux_audio {
            remux_streams.push("audio".to_string());
        }

        let remux = !remux_streams.is_empty();

        let mut nvenc_path = false;

        let conversion_opts: Vec<String> = if remux {
            // remux if possible
            log::info!(
                "remuxing {} to {} for job {}, remuxing streams: {:?}",
                self.from,
                self.to,
                job.id,
                remux_streams
            );
            self.remux_args(gpu, supported_accelerated_codecs, job, &remux_streams)
                .await
        } else {
            // extra/override args for specific formats
            match self.to {
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
                    if matches!(gpu, ConverterGPU::NVIDIA) {
                        nvenc_path = true;
                        self.nvenc_args(gpu, resolution, fps, supported_accelerated_codecs, job)
                            .await?
                    } else {
                        vec!["-strict".to_string(), "experimental".to_string()]
                    }
                }

                ConverterFormat::GIF => {
                    vec![
                        "-filter_complex".to_string(),
                        format!(
                            "fps={},scale={}:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=64[p];[s1][p]paletteuse=dither=bayer",
                            fps.min(24),
                            gif_width
                        ),
                        "-loop".to_string(),
                        "0".to_string(),
                        "-strict".to_string(),
                        "experimental".to_string(),
                    ]
                }

                ConverterFormat::DIVX => vec![
                    "-f".to_string(),
                    "avi".to_string(),
                    "-strict".to_string(),
                    "experimental".to_string(),
                ],

                ConverterFormat::SWF => vec![
                    "-f".to_string(),
                    "swf".to_string(),
                    "-strict".to_string(),
                    "experimental".to_string(),
                ],

                ConverterFormat::AMV => vec!["-strict".to_string(), "experimental".to_string()],

                ConverterFormat::RM | ConverterFormat::RMVB => {
                    warn!(
                        "encoding to {} is not supported, skipping job {}",
                        self.to, job.id
                    );
                    return Err(anyhow::anyhow!("encoding to {} is not supported", self.to));
                }

                // probably not a good practice but lol, if it generates a bad file then it's
                // likely the user doing some weird settings override lol
                _ => vec!["-strict".to_string(), "experimental".to_string()],
            }
        };

        let extra_conversion_args = conversion_opts
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();

        let mut result = if remux {
            extra_conversion_args
        } else {
            [
                extra_conversion_args,
                cap_args,
                self.to.conversion_into_args(speed, gpu, effective_bitrate),
            ]
            .concat()
        };

        // auto video codec
        if auto_video_codec && !remux && !nvenc_path {
            if let Some(preferred_video_encoder) = self
                .preferred_video_encoder(gpu, supported_accelerated_codecs)
                .await
            {
                Self::insert_codec_arg(&mut result, "-c:v", preferred_video_encoder);
            }
        }

        // auto audio codec
        if auto_audio_codec && !remux {
            if let Some(preferred_audio_encoder) = self.preferred_audio_encoder() {
                Self::insert_codec_arg(&mut result, "-c:a", preferred_audio_encoder);
            }
        }

        // apply custom settings if provided and not "auto"
        // custom fps
        if let Some(custom_fps) = Self::custom_value(&settings.fps) {
            if let Ok(fps_val) = custom_fps.parse::<u32>() {
                result.extend(["-r".to_string(), fps_val.to_string()]);
            }
        }

        // custom resolution
        if self.to != ConverterFormat::GIF {
            if let Some(custom_res) = Self::custom_value(&settings.resolution) {
                if let Some((w, h)) = custom_res.split_once('x') {
                    if w.parse::<u32>().is_ok() && h.parse::<u32>().is_ok() {
                        result.extend(["-vf".to_string(), format!("scale={}:{}", w, h)]);
                    }
                }
            }
        }

        // custom audio bitrate
        if let Some(audio_br) = Self::custom_value(&settings.audio_bitrate) {
            result.extend(["-b:a".to_string(), audio_br.to_string()]);
        }

        // custom video codec
        if let Some(video_codec) = Self::custom_value(&settings.video_codec) {
            Self::insert_codec_arg(&mut result, "-c:v", video_codec.to_string());
        }

        // custom audio codec
        if let Some(audio_codec) = Self::custom_value(&settings.audio_codec) {
            Self::insert_codec_arg(&mut result, "-c:a", audio_codec.to_string());
        }

        // custom sample rate
        if let Some(sample_r) = Self::custom_value(&settings.sample_rate) {
            result.extend(["-ar".to_string(), sample_r.to_string()]);
        }

        // for some weird case where there wasn't a specified audio codec?
        // don't actually remember what this was for
        if !result.contains(&"-c:a".to_string()) {
            result.extend(["-c:a".to_string(), "aac".to_string()]);
        }

        Ok(result)
    }

    async fn remux_args(
        &self,
        gpu: &ConverterGPU,
        supported_accelerated_codecs: &[String],
        job: &Job,
        remux: &[String],
    ) -> Vec<String> {
        let mut args = vec!["-c".to_string(), "copy".to_string()];
        let codecs = job
            .codecs()
            .await
            .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));
        let audio_codec = codecs.1.to_lowercase();
        let Some((supported_video_codecs, supported_audio_codecs)) =
            codecs::codec_support_for(self.to)
        else {
            return args;
        };

        let remux_video = remux.contains(&"video".to_string());
        let remux_audio = remux.contains(&"audio".to_string());

        if remux_video {
            args.extend(["-c:v".to_string(), "copy".to_string()]);
        } else if let Some(preferred_video_codec) = supported_video_codecs.first() {
            let default_encoder = Self::default_encoder_for_codec(preferred_video_codec);
            let encoder = self
                .accelerated_or_default_codec(
                    gpu,
                    &[*preferred_video_codec][..],
                    &default_encoder,
                    supported_accelerated_codecs,
                )
                .await;
            args.extend(["-c:v".to_string(), encoder]);
        }

        if remux_audio {
            args.extend(["-c:a".to_string(), "copy".to_string()]);
        } else if audio_codec != "none" {
            if let Some(preferred_audio_codec) = supported_audio_codecs.first() {
                args.extend([
                    "-c:a".to_string(),
                    Self::default_encoder_for_codec(preferred_audio_codec),
                ]);
            }
        }

        info!("performing remux for job {}", job.id);

        args
    }
}
