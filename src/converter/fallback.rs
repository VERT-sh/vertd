// current retry strategies supported
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    // encoder rejected the pixel format so we can try a simpler one
    PixelFormat,
    // encoder can't handle this job at all (codec, resolution, or session limits), another encoder could work
    Encoder,
    // unrecoverable, usually corrupt input
    Fatal,
}

pub fn classify(stderr: &str) -> FailureKind {
    let stderr = stderr.to_ascii_lowercase();

    const ENCODER_MARKERS: [&str; 9] = [
        "unknown encoder",
        "encoder not found",
        "encoders not supported",
        "codec not supported",
        "not within range",
        "exceeds",
        "not supported by the device",
        "openencodesessionex failed",
        "too many concurrent sessions",
    ];
    if ENCODER_MARKERS.iter().any(|m| stderr.contains(m)) {
        return FailureKind::Encoder;
    }

    const PIX_FMT_MARKERS: [&str; 9] = [
        "10 bit encode not supported",
        "incompatible pixel format",
        "yuv422p not supported",
        "yuv444p not supported",
        "hardware is lacking required capabilities",
        "failed setup for format",
        "could not find a valid device",
        "cannot perform the requested operation",
        // generic failure message
        "no capable devices found",
    ];
    if PIX_FMT_MARKERS.iter().any(|m| stderr.contains(m)) {
        return FailureKind::PixelFormat;
    }

    FailureKind::Fatal
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryAction {
    // try a different pixel format
    PixFmt(&'static str),
    // fall back to cpu encoding
    Software,
}

pub fn next_action(
    kind: FailureKind,
    encoder: &str,
    input_pix_fmt: &str,
    attempted_pix_fmts: &[String],
    hardware: bool,
) -> Option<RetryAction> {
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
        FailureKind::Fatal => None,
    }
}
