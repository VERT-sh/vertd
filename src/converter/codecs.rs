use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

use super::format::ConverterFormat;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecCatalog {
    pub video: Vec<String>,
    pub audio: Vec<String>,
    pub all: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerCodecSupport {
    pub container: String,
    pub video_codecs: Vec<String>,
    pub audio_codecs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecContainersSupport {
    pub codec: String,
    pub containers: Vec<String>,
}

pub static CONTAINER_CODEC_SUPPORT: Lazy<
    HashMap<ConverterFormat, (Vec<&'static str>, Vec<&'static str>)>,
> = Lazy::new(|| {
    HashMap::from([
        (
            ConverterFormat::MP4,
            (
                vec![
                    "h264",
                    "hevc",
                    "av1",
                    "mpeg4",
                    "mpeg1video",
                    "mpeg2video",
                    "vp9",
                    "vp8",
                    "prores",
                ],
                vec!["aac", "ac3", "eac3", "mp3", "alac", "flac", "opus", "pcm"],
            ),
        ),
        (
            ConverterFormat::MKV,
            (
                vec![
                    "h264",
                    "hevc",
                    "av1",
                    "vp9",
                    "vp8",
                    "mpeg4",
                    "mpeg2video",
                    "mpeg1video",
                    "prores",
                    "dnxhd",
                    "ffv1",
                    "huffyuv",
                    "mjpeg",
                ],
                vec![
                    "aac", "ac3", "eac3", "dts", "mp3", "flac", "opus", "vorbis", "pcm", "alac",
                    "wavpack",
                ],
            ),
        ),
        (
            ConverterFormat::MOV,
            (
                vec![
                    "h264", "hevc", "prores", "dnxhd", "mpeg4", "av1", "mjpeg", "ffv1",
                ],
                vec!["aac", "alac", "pcm", "mp3", "flac"],
            ),
        ),
        (
            ConverterFormat::MTS,
            (
                vec!["h264", "hevc", "mpeg2video"],
                vec!["aac", "ac3", "eac3", "mp2", "pcm"],
            ),
        ),
        (
            ConverterFormat::TS,
            (
                vec!["h264", "hevc", "mpeg2video", "mpeg1video", "av1"],
                vec!["aac", "ac3", "eac3", "mp2", "mp3", "dts", "truehd", "pcm"],
            ),
        ),
        (
            ConverterFormat::M2TS,
            (
                vec!["h264", "hevc", "mpeg2video", "vc1"],
                vec!["ac3", "eac3", "dts", "truehd", "pcm"],
            ),
        ),
        (
            ConverterFormat::FLV,
            (
                vec!["h264", "flv1", "vp6f", "vp6a"],
                vec!["aac", "mp3", "nellymoser", "adpcm_swf", "speex"],
            ),
        ),
        (
            ConverterFormat::F4V,
            (vec!["h264", "hevc"], vec!["aac", "mp3"]),
        ),
        (
            ConverterFormat::M4V,
            (
                vec!["h264", "hevc", "mpeg4"],
                vec!["aac", "mp3", "alac", "flac", "opus", "pcm", "ac3"],
            ),
        ),
        (
            ConverterFormat::ThreeGP,
            (
                vec!["h264", "mpeg4", "h263"],
                vec!["aac", "amr_nb", "amr_wb"],
            ),
        ),
        (
            ConverterFormat::ThreeG2,
            (
                vec!["h264", "mpeg4", "h263"],
                vec!["aac", "amr_nb", "amr_wb"],
            ),
        ),
        (
            ConverterFormat::WebM,
            (vec!["vp9", "vp8", "av1"], vec!["opus", "vorbis"]),
        ),
        (
            ConverterFormat::AVI,
            (
                vec![
                    "mpeg4",
                    "h264",
                    "mjpeg",
                    "huffyuv",
                    "dvvideo",
                    "msmpeg4v2",
                    "msmpeg4v3",
                ],
                vec!["mp3", "ac3", "pcm"],
            ),
        ),
        (
            ConverterFormat::NUT,
            (
                vec!["mpeg4", "h264", "vp9", "vp8", "ffv1"],
                vec!["mp3", "pcm", "aac", "flac"],
            ),
        ),
        (
            ConverterFormat::MPEG,
            (vec!["mpeg2video", "mpeg1video"], vec!["mp2", "mp3", "ac3"]),
        ),
        (
            ConverterFormat::MPG,
            (vec!["mpeg2video", "mpeg1video"], vec!["mp2", "mp3", "ac3"]),
        ),
        (
            ConverterFormat::VOB,
            (vec!["mpeg2video", "mpeg1video"], vec!["ac3", "mp2", "pcm"]),
        ),
        (
            ConverterFormat::MXF,
            (
                vec![
                    "mpeg2video",
                    "prores",
                    "dnxhd",
                    "h264",
                    "avc_intra",
                    "jpeg2000",
                ],
                vec!["pcm", "aac", "ac3"],
            ),
        ),
        (
            ConverterFormat::OGV,
            (
                vec!["theora", "dirac", "vp8"],
                vec!["vorbis", "opus", "flac", "speex", "pcm", "wavpack"],
            ),
        ),
        (
            ConverterFormat::ASF,
            (
                vec!["wmv2", "wmv1", "msmpeg4v3"],
                vec!["wmav2", "wmav1", "mp3"],
            ),
        ),
        (
            ConverterFormat::SWF,
            (
                vec!["flv1", "h263", "vp6f", "vp6a", "h264"],
                vec!["mp3", "nellymoser", "adpcm_swf"],
            ),
        ),
        (ConverterFormat::AMV, (vec!["amv"], vec!["adpcm_ima_amv"])),
    ])
});

pub fn all_supported_codecs() -> CodecCatalog {
    let mut video_set: BTreeSet<String> = BTreeSet::new();
    let mut audio_set: BTreeSet<String> = BTreeSet::new();

    for (video, audio) in CONTAINER_CODEC_SUPPORT.values() {
        video.iter().for_each(|codec| {
            video_set.insert((*codec).to_string());
        });
        audio.iter().for_each(|codec| {
            audio_set.insert((*codec).to_string());
        });
    }

    let video = video_set.into_iter().collect::<Vec<String>>();
    let audio = audio_set.into_iter().collect::<Vec<String>>();
    let all = video
        .iter()
        .chain(audio.iter())
        .cloned()
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect::<Vec<String>>();

    CodecCatalog { video, audio, all }
}

pub fn container_support(container: ConverterFormat) -> Option<ContainerCodecSupport> {
    let (video_codecs, audio_codecs) = CONTAINER_CODEC_SUPPORT.get(&container)?;

    Some(ContainerCodecSupport {
        container: container.to_string(),
        video_codecs: video_codecs
            .iter()
            .map(|codec| codec.to_string())
            .collect::<Vec<String>>(),
        audio_codecs: audio_codecs
            .iter()
            .map(|codec| codec.to_string())
            .collect::<Vec<String>>(),
    })
}

pub fn containers_for_codec(codec: &str) -> CodecContainersSupport {
    let codec = codec.to_lowercase();
    let mut containers = Vec::new();

    for (container, (video, audio)) in CONTAINER_CODEC_SUPPORT.iter() {
        if video.iter().any(|c| *c == codec) || audio.iter().any(|c| *c == codec) {
            containers.push(container.to_string());
        }
    }

    containers.sort();

    CodecContainersSupport { codec, containers }
}

pub fn container_supports_video_codec(container: ConverterFormat, codec: &str) -> bool {
    let Some((video, _)) = CONTAINER_CODEC_SUPPORT.get(&container) else {
        return false;
    };

    let codec = codec.to_lowercase();
    video.iter().any(|supported| codec.contains(supported))
}

pub fn container_supports_audio_codec(container: ConverterFormat, codec: &str) -> bool {
    let Some((_, audio)) = CONTAINER_CODEC_SUPPORT.get(&container) else {
        return false;
    };

    let codec = codec.to_lowercase();
    audio.iter().any(|supported| codec.contains(supported))
}
