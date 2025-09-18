use super::{gpu::ConverterGPU, speed::ConversionSpeed};
use log::warn;
use strum_macros::{Display, EnumString};

#[derive(Clone, Copy, Debug, PartialEq, EnumString, Display)]
#[strum(serialize_all = "lowercase")]
pub enum ConverterFormat {
    MP4,
    WebM,
    GIF,
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
    ) -> String {
        for codec in codecs {
            if let Ok(encoder) = gpu.get_accelerated_codec(codec).await {
                return encoder;
            }
        }
        default.to_string()
    }

    pub async fn to_args(
        &self,
        speed: &ConversionSpeed,
        gpu: &ConverterGPU,
        bitrate: u64,
        fps: u32,
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
                let encoder = self
                    .accelerated_or_default_codec(gpu, &["h264"], "libx264")
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

            ConverterFormat::GIF => {
                vec![
                   "-filter_complex".to_string(), 
                   format!(
                    "fps={},scale=800:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=64[p];[s1][p]paletteuse=dither=bayer",
                    fps.min(24)
                   )
                ]
            }

            ConverterFormat::WMV => {
                let encoder = self
                    .accelerated_or_default_codec(gpu, &["wmv2", "wmv3"], "wmv2")
                    .await;
                vec![
                    "-c:v".to_string(),
                    encoder,
                    "-c:a".to_string(),
                    "wmav2".to_string(),
                ]
            }

            ConverterFormat::WebM => {
                let encoder = self
                    .accelerated_or_default_codec(gpu, &["av1", "vp9", "vp8"], "libvpx")
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

            ConverterFormat::MPEG | ConverterFormat::MPG | ConverterFormat::VOB => vec![
                "-c:v".to_string(),
                "mpeg2video".to_string(),
                "-c:a".to_string(),
                "mp2".to_string(),
            ],

            // there is more formats that mxf supports (e.g. on cameras)
            ConverterFormat::MXF => {
                vec![
                    "-c:v".to_string(),
                    "mpeg2video".to_string(),
                    "-c:a".to_string(),
                    "pcm_s16le".to_string(),
                ]
            }

            ConverterFormat::OGV => vec![
                "-c:v".to_string(),
                "libtheora".to_string(),
                "-c:a".to_string(),
                "libvorbis".to_string(),
            ],

            ConverterFormat::RM | ConverterFormat::RMVB => {
                warn!("Encoding to RM/RMVB is not supported");
                return Err(anyhow::anyhow!("Encoding to RM/RMVB is not supported"));
            }

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
            ],
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
}
