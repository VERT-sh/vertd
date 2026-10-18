use serde::Serialize;
use std::collections::BTreeSet;
use strum::IntoEnumIterator;

use super::format::ConverterFormat;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecInfo {
    /// value vertd expects when this codec is requested (e.g. "libopus")
    pub name: String,
    /// canonical codec family (e.g. "libopus" -> "opus")
    pub canonical: String,
    /// friendly display name (e.g. "opus" -> "Opus")
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecCatalog {
    pub video: Vec<CodecInfo>,
    pub audio: Vec<CodecInfo>,
    pub all: Vec<CodecInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCodecSupport {
    pub format: String,
    pub video_codecs: Vec<CodecInfo>,
    pub audio_codecs: Vec<CodecInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecFormatsSupport {
    pub codec: String,
    pub formats: Vec<String>,
}

#[rustfmt::skip]
pub fn codec_support_for(
    format: ConverterFormat,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match format {
        ConverterFormat::MP4 => Some((
            &["h264", "hevc", "av1", "mpeg4", "vp9", "mpeg2video", "mpeg1video"],
            &["aac", "mp3", "ac3", "eac3", "libopus", "alac", "flac"],
        )),
        ConverterFormat::WebM => Some((
            &["vp9", "vp8", "av1"],
            &["libopus", "libvorbis"],
        )),
        ConverterFormat::AVI => Some((
            &["mpeg4", "msmpeg4v3", "msmpeg4v2", "h264", "mjpeg", "dvvideo", "huffyuv"],
            &["mp3", "ac3", "pcm_s16le", "pcm_s24le"],
        )),
        ConverterFormat::MKV => Some((
            &["h264", "hevc", "av1", "vp9", "vp8", "mpeg2video", "mpeg4", "prores", "dnxhd", "ffv1", "huffyuv", "mjpeg", "mpeg1video"],
            &["aac", "mp3", "ac3", "eac3", "dts", "libopus", "libvorbis", "flac", "alac", "wavpack", "pcm_s16le", "pcm_s24le", "pcm_s32le", "pcm_f32le", "pcm_f64le", "pcm_mulaw", "pcm_alaw", "pcm_u8", "pcm_f16le", "pcm_f24le", "pcm_f32be", "pcm_f64be", "pcm_s16be", "pcm_s24be", "pcm_s32be", "pcm_s64be", "pcm_s64le", "pcm_u16be", "pcm_u24be", "pcm_u24le", "pcm_u32be", "pcm_u32le"],
        )),
        ConverterFormat::MOV => Some((
            &["h264", "hevc", "prores", "dnxhd", "mpeg4", "mjpeg", "ffv1"],
            &["aac", "alac", "mp3", "flac", "pcm_s16le", "pcm_s24le", "pcm_s32le", "pcm_f32le", "pcm_f64le"],
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
            &["aac", "ac3", "mp2", "mp3", "dts", "eac3", "truehd", "pcm_s16le"],
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
        ConverterFormat::F4V => Some((
            &["h264"],
            &["aac", "mp3"],
        )),
        ConverterFormat::VOB => Some((
            &["mpeg2video", "mpeg1video"],
            &["ac3", "mp2", "pcm_dvd", "pcm_s16le"],
        )),
        ConverterFormat::M4V => Some((
            &["h264", "mpeg4"],
            &["aac", "ac3", "alac", "flac"],
        )),
        ConverterFormat::ThreeGP => Some((
            &["h264", "mpeg4", "h263"],
            &["aac", "amr_nb", "amr_wb"],
        )),
        ConverterFormat::ThreeG2 => Some((
            &["h264", "mpeg4", "h263"],
            &["aac", "amr_nb", "amr_wb"],
        )),
        ConverterFormat::MXF => Some((
            &["prores", "dnxhd", "h264", "mpeg2video", "avc_intra", "jpeg2000"],
            &["pcm_s24le", "pcm_s16le", "pcm_s32le", "pcm_f32le", "pcm_f64le"],
        )),
        ConverterFormat::GXF => Some((
            &["mpeg2video", "dvvideo", "mjpeg", "mpeg1video"],
            &["pcm_s16le", "pcm_s24le", "ac3"]
        )),
        ConverterFormat::OGV | ConverterFormat::OGX => Some((
            &["theora", "vp8"],
            &["libvorbis", "libopus", "flac", "speex"],
        )),
        ConverterFormat::SWF => Some((
            &["flv1", "flashsv", "mjpeg"],
            &["mp3"],
        )),
        ConverterFormat::AMV => Some((
            &["amv"],
            &["adpcm_ima_amv"],
        )),
        ConverterFormat::ASF => Some((
            &["wmv2", "wmv1", "msmpeg4v3"],
            &["wmav2", "wmav1", "mp3"],
        )),
        ConverterFormat::NUT => Some((
            &["h264", "mpeg4", "vp9", "vp8", "ffv1", "mjpeg", "huffyuv", "h263", "prores", "mpeg2video", "h261", "msvideo1", "rv10", "rv20", "snow", "svq1", "ljpeg"],
            &["aac", "mp3", "ac3", "eac3", "dts", "libopus", "libvorbis", "flac", "alac", "wavpack", "pcm_s16le", "pcm_s24le", "pcm_s32le", "pcm_f32le", "pcm_f64le", "pcm_mulaw", "pcm_alaw", "pcm_u8", "pcm_f16le", "pcm_f24le", "pcm_f32be", "pcm_f64be", "pcm_s16be", "pcm_s24be", "pcm_s32be", "pcm_s64be", "pcm_s64le", "pcm_u16be", "pcm_u16le", "pcm_u24be", "pcm_u24le", "pcm_u32be", "pcm_u32le"],
        )),
        ConverterFormat::H264 => Some((
            &["h264"],
            &[],
        )),
        ConverterFormat::DIVX => Some((
            &["mpeg4", "h264", "hevc"],
            &["mp3", "ac3", "aac", "pcm_s16le"],
        )),
        ConverterFormat::GIF => Some((
            &["gif"],
            &[],
        )),
        ConverterFormat::APNG => Some((
            &["apng"],
            &[],
        )),
        ConverterFormat::WEBP => Some((
            &["webp"],
            &[],
        )),
        ConverterFormat::WMV => Some((
            &["wmv2", "wmv1"],
            &["wmav2"],
        )),
        ConverterFormat::IVF => Some((
            &["vp8", "vp9", "av1"],
            &[],
        )),
        ConverterFormat::Y4M => Some((
            &["wrapped_avframe", "rawvideo"],
            &[],
        )),
        ConverterFormat::RM | ConverterFormat::RMVB | ConverterFormat::MediaSequence => None,
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

    let all = video_set
        .iter()
        .chain(audio_set.iter())
        .cloned()
        .collect::<BTreeSet<String>>();
    let to_info = |codecs: BTreeSet<String>| -> Vec<CodecInfo> {
        codecs.into_iter().map(|codec| codec_info(&codec)).collect()
    };

    CodecCatalog {
        video: to_info(video_set),
        audio: to_info(audio_set),
        all: to_info(all),
    }
}

pub fn format_support(format: ConverterFormat) -> Option<FormatCodecSupport> {
    let (video_codecs, audio_codecs) = codec_support_for(format)?;

    Some(FormatCodecSupport {
        format: format.to_string(),
        video_codecs: video_codecs.iter().map(|codec| codec_info(codec)).collect(),
        audio_codecs: audio_codecs.iter().map(|codec| codec_info(codec)).collect(),
    })
}

pub fn formats_for_codec(codec: &str) -> CodecFormatsSupport {
    let codec = codec.to_lowercase();
    let mut formats = Vec::new();

    for format in ConverterFormat::iter() {
        let Some((video, audio)) = codec_support_for(format) else {
            continue;
        };

        if video
            .iter()
            .chain(audio.iter())
            .any(|c| canonical_codec(c) == canonical_codec(&codec))
        {
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
    video
        .iter()
        .any(|supported| canonical_codec(&codec) == canonical_codec(supported))
}

pub fn support_audio_codec(format: ConverterFormat, codec: &str) -> bool {
    let Some((_, audio)) = codec_support_for(format) else {
        return false;
    };

    let codec = codec.to_lowercase();
    audio
        .iter()
        .any(|supported| canonical_codec(&codec) == canonical_codec(supported))
}

pub fn codec_info(codec: &str) -> CodecInfo {
    let canonical = canonical_codec(codec).to_string();
    let label = codec_label(&canonical);

    CodecInfo {
        name: codec.to_string(),
        canonical,
        label,
    }
}

// friendly display name for a canonical codec name
fn codec_label(canonical: &str) -> String {
    let label = match canonical {
        // video
        "h264" => "H264 (AVC)",
        "hevc" => "H265 (HEVC)",
        "vp8" => "VP8",
        "vp9" => "VP9",
        "av1" => "AV1",
        "prores" => "Apple ProRes",
        "mpeg4" => "MPEG-4 Part 2",
        "mpeg1video" => "MPEG-1",
        "mpeg2video" => "MPEG-2",
        "mjpeg" => "Motion JPEG",
        "ljpeg" => "Lossless JPEG",
        "dnxhd" => "Avid DNxHD",
        "ffv1" => "FFV1",
        "huffyuv" => "HuffYUV",
        "theora" => "Theora",
        "h263" => "H.263",
        "h261" => "H.261",
        "flv1" => "Sorenson Spark (FLV1)",
        "wmv1" => "Windows Media Video 7",
        "wmv2" => "Windows Media Video 8",
        "msmpeg4v2" => "Microsoft MPEG-4 v2",
        "msmpeg4v3" => "Microsoft MPEG-4 v3",
        "dvvideo" => "DV",
        "svq1" => "Sorenson Video 1",
        "snow" => "Snow",
        "amv" => "AMV",
        "flashsv" => "Flash Screen Video",
        "apng" => "APNG",
        "webp" => "WebP",
        "gif" => "GIF",

        // audio
        "aac" => "AAC",
        "mp3" => "MP3",
        "mp2" => "MP2",
        "ac3" => "AC-3 (Dolby Digital)",
        "eac3" => "E-AC-3 (Dolby Digital Plus)",
        "dts" => "DTS",
        "truehd" => "Dolby TrueHD",
        "opus" => "Opus",
        "vorbis" => "Vorbis",
        "flac" => "FLAC",
        "alac" => "ALAC",
        "wavpack" => "WavPack",
        "tta" => "TTA",
        "speex" => "Speex",
        "nellymoser" => "Nellymoser",
        "adpcm_swf" => "ADPCM (SWF)",
        "adpcm_ima_amv" => "ADPCM (AMV)",
        "wmav1" => "Windows Media Audio 7",
        "wmav2" => "Windows Media Audio 8",
        other => other,
    };

    label.to_string()
}

fn canonical_codec(codec: &str) -> &str {
    match codec {
        "liboapv" => "apv",
        "libaom-av1" | "librav1e" | "libsvtav1" => "av1",
        "libxavs2" => "avs2",
        "libxavs" => "cavs",
        "libxeve" => "evc",
        "libx264" | "libx264rgb" | "libopenh264" | "h264_qsv" | "h264_amf" | "h264_nvenc"
        | "h264_vaapi" | "h264_videotoolbox" => "h264",
        "libx265" | "libkvazaar" | "hevc_qsv" | "hevc_amf" | "hevc_nvenc" | "hevc_vaapi"
        | "hevc_videotoolbox" => "hevc",
        "libopenjpeg" | "jpeg2000_qsv" => "jpeg2000",
        "libjxl" => "jpegxl",
        "libjxl_anim" => "jpegxl_anim",
        "libxvid" | "mpeg4_qsv" | "mpeg4_omx" => "mpeg4",
        "mpeg2video_qsv" | "mpeg2_vaapi" | "mpeg2_nvenc" => "mpeg2video",
        "mpeg1_qsv" => "mpeg1video",
        "libvpx" | "vp8_vaapi" | "vp8_qsv" | "vp8_videotoolbox" => "vp8",
        "libvpx-vp9" | "vp9_qsv" | "vp9_vaapi" | "vp9_videotoolbox" => "vp9",
        "libvvenc" => "vvc",
        "libwebp" | "libwebp_anim" => "webp",
        "prores_ks" | "prores_aw" | "prores_videotoolbox" => "prores",
        "dnxhd_vaapi" => "dnxhd",
        "mjpeg_qsv" | "mjpeg_vaapi" => "mjpeg",
        "libopencore_amrnb" => "amr_nb",
        "libvo_amrwbenc" => "amr_wb",
        "libtwolame" => "mp2",
        "libmp3lame" | "libshine" | "mp3_mf" | "mp3_amf" => "mp3",
        "libfdk_aac" | "aac_mf" => "aac",
        "libopus" => "opus",
        "libvorbis" => "vorbis",
        "libspeex" => "speex",
        "wmav2_mf" => "wmav2",
        // try lib(x) then name itself
        other => other.strip_prefix("lib").unwrap_or(other),
    }
}
