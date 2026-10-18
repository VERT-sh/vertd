// current retry strategies supported
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    // encoder rejected the pixel format so we can try a simpler one
    PixelFormat,
    // encoder can't handle this job at all (codec, resolution, or session limits), another encoder could work
    Encoder,
    // the hardware decoder could not handle the input, software decoding may still work
    Decode,
    // dimensions are not a multiple of the encoder's chroma alignment
    Alignment,
    // timestamps are missing, negative, or out of order
    Timestamps,
    // ffprobe could not read enough of the input to build a command
    Probe,
    // the muxer rejected an audio stream we tried to copy
    AudioCopy,
    // the muxer rejected a video stream we tried to copy
    VideoCopy,
    // unrecoverable, usually corrupt input
    Fatal,
}

// overrides applied to the ffmpeg command of a retry
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetryOverrides {
    // force this pixel format on the output encoder
    pub pix_fmt: Option<&'static str>,
    // decode in software while still using the hardware encoder
    pub sw_decode: bool,
    // pad to the encoder's chroma alignment instead of failing on odd dimensions
    pub pad_alignment: bool,
    // force timestamp regeneration and normalisation
    pub fix_timestamps: bool,
    // encode audio instead of copying it
    pub transcode_audio: bool,
    // encode video instead of copying it
    pub transcode_video: bool,
}

pub const MAX_ATTEMPTS: usize = 12;

const BENIGN_MARKERS: &[&str] = &[
    "failed setup for format",
    "hwaccel initialisation returned error",
    "format not usable, retrying",
    "incompatible pixel format",
    "hardware is lacking required capabilities",
    "not within range",
];

// drop benign lines so the surviving text is the actual terminal cause
fn without_benign(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            !BENIGN_MARKERS.iter().any(|marker| line.contains(marker))
        })
        .collect::<Vec<&str>>()
        .join("\n")
        .to_ascii_lowercase()
}

pub fn classify(stderr: &str, audio_codecs: &[String]) -> FailureKind {
    let stderr = without_benign(stderr);

    if let Some(kind) = muxer_rejection(&stderr, audio_codecs) {
        return kind;
    }

    if alignment_failure(&stderr) {
        return FailureKind::Alignment;
    }
    if timestamp_failure(&stderr) {
        return FailureKind::Timestamps;
    }

    const ENCODER_MARKERS: [&str; 6] = [
        "unknown encoder",
        "encoder not found",
        "driver does not support the required nvenc api version",
        "openencodesessionex failed",
        "error creating a mfx session",
        "error initializing a mfx session",
    ];
    if ENCODER_MARKERS.iter().any(|m| stderr.contains(m)) {
        return FailureKind::Encoder;
    }

    const ENVIRONMENT_MARKERS: [&str; 3] = [
        "device creation failed",
        "no va display found",
        "no cuda capable devices",
    ];
    if ENVIRONMENT_MARKERS.iter().any(|m| stderr.contains(m)) {
        return FailureKind::Encoder;
    }

    const DECODER_FATAL_MARKERS: [&str; 3] = [
        "error while opening decoder",
        "failed loading nvcuvid",
        "no device available for decoder",
    ];
    if DECODER_FATAL_MARKERS.iter().any(|m| stderr.contains(m)) {
        return FailureKind::Decode;
    }

    if stderr.contains("exceeds") {
        return FailureKind::Encoder;
    }

    const PIX_FMT_MARKERS: [&str; 14] = [
        "10 bit encode not supported",
        "yuv422p not supported",
        "yuv444p not supported",
        "invalid input pixel format",
        "invalid input pixfmt",
        "current pixel format is unsupported",
        "current profile is unsupported",
        "selected ratecontrol mode is unsupported",
        "rc mode (supported modes",
        "not supported by the device",
        "no usable encoding profile",
        // generic failure message
        "provided device doesn't support required nvenc features",
        "no capable devices found",
        "could not find a valid device",
    ];
    if PIX_FMT_MARKERS.iter().any(|m| stderr.contains(m)) {
        return FailureKind::PixelFormat;
    }

    const AUDIO_MARKERS: [&str; 3] = [
        "does not support sample rate",
        "specified sample rate",
        "specified channel layout",
    ];
    if AUDIO_MARKERS.iter().any(|m| stderr.contains(m)) {
        return FailureKind::AudioCopy;
    }

    FailureKind::Fatal
}

// failures during probing
pub fn classify_probe(stderr: &str) -> FailureKind {
    let stderr = stderr.to_ascii_lowercase();

    // "Could not find codec parameters for stream 0 ..." followed by
    // "Consider increasing the value for the 'analyzeduration' and 'probesize' options"
    if stderr.contains("could not find codec parameters")
        || stderr.contains("consider increasing the value for the 'analyzeduration'")
        || stderr.contains("could not determine codec type")
    {
        return FailureKind::Probe;
    }

    FailureKind::Fatal
}

fn alignment_failure(stderr: &str) -> bool {
    // "width not divisible by 2 (641x361)" / "height not divisible by 2"
    stderr.contains("not divisible by 2")
        || stderr.contains("not divisible by 4")
        || stderr.contains("width not divisible")
        || stderr.contains("height not divisible")
}

fn timestamp_failure(stderr: &str) -> bool {
    // "Application provided invalid, non monotonically increasing dts to muxer"
    // "Can't write packet with unknown timestamp"
    stderr.contains("non monotonically increasing dts")
        || stderr.contains("unknown timestamp")
        || stderr.contains("has no timestamps")
        || stderr.contains("invalid dts")
}

fn muxer_rejection(stderr: &str, audio_codecs: &[String]) -> Option<FailureKind> {
    // "audio codec %s not compatible with flv" / "video codec %s not compatible with flv"
    if stderr.contains("not compatible with flv") {
        return if stderr.contains("audio codec") {
            Some(FailureKind::AudioCopy)
        } else if stderr.contains("video codec") {
            Some(FailureKind::VideoCopy)
        } else {
            None
        };
    }

    // "muxer does not support %s stream of type %s" / "tag %s incompatible"
    if stderr.contains("muxer does not support") && stderr.contains("stream of type") {
        return Some(if stderr.contains("stream of type audio") {
            FailureKind::AudioCopy
        } else {
            FailureKind::VideoCopy
        });
    }

    if stderr.contains("tag ") && stderr.contains("incompatible with output codec id") {
        return Some(FailureKind::VideoCopy);
    }

    let codec = tag_rejected_codec(stderr)?;
    let is_audio = audio_codecs
        .iter()
        .any(|known| known.eq_ignore_ascii_case(codec));
    Some(if is_audio {
        FailureKind::AudioCopy
    } else {
        FailureKind::VideoCopy
    })
}

// codec name from "could not find tag for codec <name> in stream #n"
fn tag_rejected_codec(stderr: &str) -> Option<&str> {
    let rest = stderr.split_once("could not find tag for codec ")?.1;
    let name = rest
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()?;
    (!name.is_empty()).then_some(name)
}

// detect encoder from ffmpeg stderr
pub fn detect_encoder(stderr: &str) -> Option<&'static str> {
    const ENCODERS: [&str; 6] = [
        "h264_nvenc",
        "hevc_nvenc",
        "av1_nvenc",
        "libx264",
        "libx265",
        "libsvtav1",
    ];
    let stderr = stderr.to_ascii_lowercase();
    ENCODERS.into_iter().find(|name| stderr.contains(name))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAction {
    // try a different pixel format
    PixFmt(&'static str),
    // fall back to cpu encoding
    Software,
    // keep the encoder but decode in software
    SoftwareDecode,
    // pad odd dimensions to the encoder's alignment
    PadAlignment,
    // regenerate and normalise timestamps
    FixTimestamps,
    // re-probe with a larger probesize/analyzeduration
    DeeperProbe,
    // stop copying audio and encode it
    TranscodeAudio,
    // stop copying video and encode it
    TranscodeVideo,
}

pub fn next_action(
    kind: FailureKind,
    encoder: &str,
    input_pix_fmt: &str,
    attempted_pix_fmts: &[String],
    gpu: crate::converter::gpu::ConverterGPU,
    applied: RetryOverrides,
) -> Option<RetryAction> {
    let hardware = gpu != crate::converter::gpu::ConverterGPU::CPU;

    match kind {
        FailureKind::PixelFormat => {
            let next = crate::converter::pixfmt::retry_pix_fmts(
                encoder,
                input_pix_fmt,
                attempted_pix_fmts,
            )
            .into_iter()
            .next();

            match next {
                Some(pix_fmt) => Some(RetryAction::PixFmt(pix_fmt)),
                // no more pixel formats to try, fall back to software if hardware was being used
                None if hardware => Some(RetryAction::Software),
                // give up </3
                None => None,
            }
        }
        FailureKind::Encoder if hardware => Some(RetryAction::Software),
        FailureKind::Encoder => None,
        FailureKind::Decode if gpu.supports_software_decode() && !applied.sw_decode => {
            Some(RetryAction::SoftwareDecode)
        }
        FailureKind::Decode if hardware => Some(RetryAction::Software),
        FailureKind::Decode => None,
        FailureKind::Alignment if !applied.pad_alignment => Some(RetryAction::PadAlignment),
        FailureKind::Alignment => None,
        FailureKind::Timestamps if !applied.fix_timestamps => Some(RetryAction::FixTimestamps),
        FailureKind::Timestamps => None,
        FailureKind::Probe => Some(RetryAction::DeeperProbe),
        // each copy failure only gets one retry
        FailureKind::AudioCopy if !applied.transcode_audio => Some(RetryAction::TranscodeAudio),
        FailureKind::AudioCopy => None,
        FailureKind::VideoCopy if !applied.transcode_video => Some(RetryAction::TranscodeVideo),
        FailureKind::VideoCopy => None,
        FailureKind::Fatal => None,
    }
}
