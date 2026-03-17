use serde::Serialize;
use std::collections::BTreeSet;
use strum::IntoEnumIterator;

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

pub fn codec_support_for(
    format: ConverterFormat,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match format {
        ConverterFormat::MP4 => Some((
            &[
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
            &["aac", "ac3", "eac3", "mp3", "alac", "flac", "opus", "pcm"],
        )),
        ConverterFormat::WebM => Some((&["vp9", "vp8", "av1"], &["opus", "vorbis"])),
        ConverterFormat::AVI => Some((
            &[
                "mpeg4",
                "h264",
                "mjpeg",
                "huffyuv",
                "dvvideo",
                "msmpeg4v2",
                "msmpeg4v3",
            ],
            &["mp3", "ac3", "pcm"],
        )),
        ConverterFormat::MKV => Some((
            &[
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
            &[
                "aac", "ac3", "eac3", "dts", "mp3", "flac", "opus", "vorbis", "pcm", "alac",
                "wavpack",
            ],
        )),
        ConverterFormat::MOV => Some((
            &[
                "h264", "hevc", "prores", "dnxhd", "mpeg4", "av1", "mjpeg", "ffv1",
            ],
            &["aac", "alac", "pcm", "mp3", "flac"],
        )),
        ConverterFormat::MTS => Some((
            &["h264", "hevc", "mpeg2video"],
            &["aac", "ac3", "eac3", "mp2", "pcm"],
        )),
        ConverterFormat::TS => Some((
            &["h264", "hevc", "mpeg2video", "mpeg1video", "av1"],
            &["aac", "ac3", "eac3", "mp2", "mp3", "dts", "truehd", "pcm"],
        )),
        ConverterFormat::M2TS => Some((
            &["h264", "hevc", "mpeg2video", "vc1"],
            &["ac3", "eac3", "dts", "truehd", "pcm"],
        )),
        ConverterFormat::MPEG => Some((&["mpeg2video", "mpeg1video"], &["mp2", "mp3", "ac3"])),
        ConverterFormat::MPG => Some((&["mpeg2video", "mpeg1video"], &["mp2", "mp3", "ac3"])),
        ConverterFormat::FLV => Some((
            &["h264", "flv1", "h263"],
            &["aac", "mp3", "nellymoser", "adpcm_swf", "speex"],
        )),
        ConverterFormat::F4V => Some((&["h264", "hevc"], &["aac", "mp3"])),
        ConverterFormat::VOB => Some((&["mpeg2video", "mpeg1video"], &["ac3", "mp2", "pcm"])),
        ConverterFormat::M4V => Some((
            &["h264", "hevc", "mpeg4"],
            &["aac", "mp3", "alac", "flac", "opus", "pcm", "ac3"],
        )),
        ConverterFormat::ThreeGP => {
            Some((&["h264", "mpeg4", "h263"], &["aac", "amr_nb", "amr_wb"]))
        }
        ConverterFormat::ThreeG2 => {
            Some((&["h264", "mpeg4", "h263"], &["aac", "amr_nb", "amr_wb"]))
        }
        ConverterFormat::MXF => Some((
            &[
                "mpeg2video",
                "prores",
                "dnxhd",
                "h264",
                "avc_intra",
                "jpeg2000",
            ],
            &["pcm", "aac", "ac3"],
        )),
        ConverterFormat::OGV => Some((
            &["theora", "dirac", "vp8"],
            &["vorbis", "opus", "flac", "speex", "pcm", "wavpack"],
        )),
        ConverterFormat::SWF => Some((
            &["flv1", "h263", "h264"],
            &["mp3", "nellymoser", "adpcm_swf"],
        )),
        ConverterFormat::AMV => Some((&["amv"], &["adpcm_ima_amv"])),
        ConverterFormat::ASF => Some((&["wmv2", "wmv1", "msmpeg4v3"], &["wmav2", "wmav1", "mp3"])),
        ConverterFormat::NUT => Some((
            &["mpeg4", "h264", "vp9", "vp8", "ffv1"],
            &["mp3", "pcm", "aac", "flac"],
        )),
        ConverterFormat::H264 => Some((
            &["h264"],
            &["aac", "ac3", "eac3", "mp3", "alac", "flac", "opus", "pcm"],
        )),
        ConverterFormat::DIVX => Some((&["h264", "mpeg4", "h265"], &["mp3", "aac", "ac3", "pcm"])),
        ConverterFormat::GIF => Some((&["gif"], &[])),
        ConverterFormat::APNG => Some((&["apng"], &[])),
        ConverterFormat::WEBP => Some((&["webp"], &[])),
        ConverterFormat::WMV => Some((&["wmv2", "wmv1"], &["wmav2"])),
        ConverterFormat::RM | ConverterFormat::RMVB => None,
    }
}

pub fn all_supported_codecs() -> CodecCatalog {
    let mut video_set: BTreeSet<String> = BTreeSet::new();
    let mut audio_set: BTreeSet<String> = BTreeSet::new();

    for format in ConverterFormat::iter() {
        let Some((video, audio)) = codec_support_for(format) else {
            continue;
        };

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
    let (video_codecs, audio_codecs) = codec_support_for(container)?;

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

    for container in ConverterFormat::iter() {
        let Some((video, audio)) = codec_support_for(container) else {
            continue;
        };

        if video.iter().any(|c| *c == codec) || audio.iter().any(|c| *c == codec) {
            containers.push(container.to_string());
        }
    }

    containers.sort();

    CodecContainersSupport { codec, containers }
}

pub fn container_supports_video_codec(container: ConverterFormat, codec: &str) -> bool {
    let Some((video, _)) = codec_support_for(container) else {
        return false;
    };

    let codec = codec.to_lowercase();
    video.iter().any(|supported| codec.contains(supported))
}

pub fn container_supports_audio_codec(container: ConverterFormat, codec: &str) -> bool {
    let Some((_, audio)) = codec_support_for(container) else {
        return false;
    };

    let codec = codec.to_lowercase();
    audio.iter().any(|supported| codec.contains(supported))
}
