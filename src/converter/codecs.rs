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
pub struct FormatCodecSupport {
    pub format: String,
    pub video_codecs: Vec<String>,
    pub audio_codecs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecFormatsSupport {
    pub codec: String,
    pub formats: Vec<String>,
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
                "vp9",
                "mpeg2video",
                "mpeg1video",
                "prores",
            ],
            &["aac", "mp3", "ac3", "eac3", "libopus", "alac", "flac"],
        )),
        ConverterFormat::WebM => Some((&["vp9", "vp8", "av1"], &["libopus", "libvorbis"])),
        ConverterFormat::AVI => Some((
            &[
                "mpeg4",
                "msmpeg4v3",
                "msmpeg4v2",
                "h264",
                "mjpeg",
                "dvvideo",
                "huffyuv",
            ],
            &["mp3", "ac3", "pcm_s16le", "pcm_s24le"],
        )),
        ConverterFormat::MKV => Some((
            &[
                "h264",
                "hevc",
                "av1",
                "vp9",
                "vp8",
                "mpeg2video",
                "mpeg4",
                "prores",
                "dnxhd",
                "ffv1",
                "huffyuv",
                "mjpeg",
                "mpeg1video",
            ],
            &[
                "aac",
                "mp3",
                "ac3",
                "eac3",
                "dts",
                "libopus",
                "libvorbis",
                "flac",
                "alac",
                "wavpack",
                "pcm_s16le",
                "pcm_s24le",
                "pcm_s32le",
                "pcm_f32le",
                "pcm_f64le",
            ],
        )),
        ConverterFormat::MOV => Some((
            &["h264", "hevc", "prores", "dnxhd", "mpeg4", "mjpeg", "ffv1"],
            &[
                "aac",
                "alac",
                "mp3",
                "flac",
                "pcm_s16le",
                "pcm_s24le",
                "pcm_s32le",
                "pcm_f32le",
                "pcm_f64le",
            ],
        )),
        ConverterFormat::M2TS => Some((
            &["h264", "mpeg2video"],
            &["truehd", "dts", "ac3", "eac3", "pcm_bluray"],
        )),

        ConverterFormat::MTS => Some((
            &["h264", "mpeg2video", "hevc"],
            &["ac3", "aac", "pcm_bluray", "mp2", "eac3"],
        )),

        ConverterFormat::TS => Some((
            &["h264", "hevc", "mpeg2video", "av1", "mpeg1video"],
            &[
                "aac",
                "ac3",
                "mp2",
                "mp3",
                "dts",
                "eac3",
                "truehd",
                "pcm_s16le",
            ],
        )),
        ConverterFormat::MPEG => Some((
            &["mpeg2video", "mpeg1video"],
            &["ac3", "mp2", "mp3", "pcm_dvd"],
        )),
        ConverterFormat::MPG => Some((
            &["mpeg2video", "mpeg1video"],
            &["ac3", "mp2", "mp3", "pcm_dvd"],
        )),
        ConverterFormat::FLV => Some((
            &["h264", "flv1", "h263"],
            &["aac", "mp3", "nellymoser", "speex", "adpcm_swf"],
        )),
        ConverterFormat::F4V => Some((&["h264"], &["aac", "mp3"])),
        ConverterFormat::VOB => Some((
            &["mpeg2video", "mpeg1video"],
            &["ac3", "mp2", "pcm_dvd", "pcm_s16le"],
        )),
        ConverterFormat::M4V => Some((
            &["h264", "hevc", "mpeg4"],
            &["aac", "ac3", "alac", "mp3", "flac", "libopus", "pcm_s16le"],
        )),
        ConverterFormat::ThreeGP => {
            Some((&["h264", "mpeg4", "h263"], &["aac", "amr_nb", "amr_wb"]))
        }
        ConverterFormat::ThreeG2 => {
            Some((&["h264", "mpeg4", "h263"], &["aac", "amr_nb", "amr_wb"]))
        }
        ConverterFormat::MXF => Some((
            &[
                "prores",
                "dnxhd",
                "h264",
                "mpeg2video",
                "avc_intra",
                "jpeg2000",
            ],
            &[
                "pcm_s24le",
                "pcm_s16le",
                "pcm_s32le",
                "pcm_f32le",
                "pcm_f64le",
                "aac",
                "ac3",
            ],
        )),
        ConverterFormat::OGV => Some((
            &["theora", "vp8", "dirac"],
            &[
                "libvorbis",
                "libopus",
                "flac",
                "speex",
                "pcm_s16le",
                "wavpack",
            ],
        )),
        ConverterFormat::SWF => Some((&["flv1", "flashsv", "mjpeg"], &["mp3"])),
        ConverterFormat::AMV => Some((&["amv"], &["adpcm_ima_amv"])),
        ConverterFormat::ASF => Some((&["wmv2", "wmv1", "msmpeg4v3"], &["wmav2", "wmav1", "mp3"])),
        ConverterFormat::NUT => Some((
            &["h264", "mpeg4", "vp9", "vp8", "ffv1"],
            &[
                "aac",
                "mp3",
                "flac",
                "pcm_s16le",
                "pcm_s24le",
                "alac",
                "libopus",
                "libvorbis",
            ],
        )),
        ConverterFormat::H264 => Some((
            &["h264"],
            &[
                "aac",
                "mp3",
                "ac3",
                "eac3",
                "libopus",
                "alac",
                "flac",
                "pcm_s16le",
            ],
        )),
        ConverterFormat::DIVX => Some((
            &["mpeg4", "h264", "hevc"],
            &["mp3", "ac3", "aac", "pcm_s16le"],
        )),
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

pub fn format_support(format: ConverterFormat) -> Option<FormatCodecSupport> {
    let (video_codecs, audio_codecs) = codec_support_for(format)?;

    Some(FormatCodecSupport {
        format: format.to_string(),
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

pub fn formats_for_codec(codec: &str) -> CodecFormatsSupport {
    let codec = codec.to_lowercase();
    let mut formats = Vec::new();

    for format in ConverterFormat::iter() {
        let Some((video, audio)) = codec_support_for(format) else {
            continue;
        };

        if video.iter().any(|c| *c == codec) || audio.iter().any(|c| *c == codec) {
            formats.push(format.to_string());
        }
    }

    formats.sort();

    CodecFormatsSupport { codec, formats }
}

pub fn support_video_codec(format: ConverterFormat, codec: &str) -> bool {
    let Some((video, _)) = codec_support_for(format) else {
        return false;
    };

    let codec = codec.to_lowercase();
    video.iter().any(|supported| codec.contains(supported))
}

pub fn support_audio_codec(format: ConverterFormat, codec: &str) -> bool {
    let Some((_, audio)) = codec_support_for(format) else {
        return false;
    };

    let codec = codec.to_lowercase();
    audio.iter().any(|supported| codec.contains(supported))
}
