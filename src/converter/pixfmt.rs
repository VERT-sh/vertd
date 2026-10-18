pub fn pix_fmt_bit_depth(pix_fmt: &str) -> u8 {
    if pix_fmt.starts_with("bayer") {
        return 8;
    }

    // "msb" is a marker for 10-bit 4:4:4, but the "10" is still the depth
    // e.g. "yuv444p10msble" -> "yuv444p10le" -> 10
    let normalized = pix_fmt.replace("msb", "");

    const DEPTHS: [(&str, u8); 20] = [
        ("128le", 16),
        ("128be", 16),
        ("96le", 16),
        ("96be", 16),
        ("64le", 16),
        ("64be", 16),
        ("48le", 16),
        ("48be", 16),
        ("32le", 16),
        ("32be", 16),
        ("16le", 16),
        ("16be", 16),
        ("14le", 14),
        ("14be", 14),
        ("12le", 12),
        ("12be", 12),
        ("10le", 10),
        ("10be", 10),
        ("9le", 9),
        ("9be", 9),
    ];
    for (suffix, depth) in DEPTHS {
        if normalized.ends_with(suffix) {
            return depth;
        }
    }

    // names whose trailing digits are not the depth, but which are still 10-bit
    const IMPLICIT_10BIT: [&str; 8] = [
        "nv20le", "nv20be", "xv30le", "xv30be", "xv36le", "xv36be", "v30xle", "v30xbe",
    ];
    if IMPLICIT_10BIT.iter().any(|s| normalized.ends_with(s)) {
        return 10;
    }

    8
}

pub fn is_high_bit_depth(pix_fmt: &str) -> bool {
    pix_fmt_bit_depth(pix_fmt) > 8
}

// pixel formats to try in order when an encoder refuses the first attempt
pub const PIX_FMT_LADDER: [&str; 7] = [
    // 4:4:4
    "yuv444p10msble",
    "yuv444p",
    // 4:2:2
    "yuv422p10le",
    "yuv422p",
    // 4:2:0
    "p010le",
    "yuv420p10le",
    // universal
    "yuv420p",
];

// highest bit depth supported by the encoder
// vaapi returns None because the ladder cannot reach the encoder at all, so it is always unconstrained
fn max_bit_depth(encoder: &str) -> Option<u8> {
    match encoder {
        // only Blackwell cards (RTX 50xx) support 10-bit h264
        "h264_nvenc" => Some(8),
        "hevc_nvenc" | "av1_nvenc" => Some(10),
        "libx264" => Some(10),
        "libx265" | "libaom-av1" => Some(12),
        "libsvtav1" => Some(10),
        "libvpx-vp9" | "libvpx" => Some(10),
        "prores_ks" => Some(10),
        _ => None,
    }
}

// pixel formats that are known to be accepted by the encoder
fn encoder_formats(encoder: &str) -> Option<&'static [&'static str]> {
    match encoder {
        "h264_nvenc" => Some(&[
            "yuv420p", "nv12", "yuv444p", "bgr0", "bgra", "rgb0", "rgba", "gbrp",
        ]),
        "hevc_nvenc" => Some(&[
            "yuv420p",
            "nv12",
            "p010le",
            "yuv420p10le",
            "yuv444p",
            "yuv444p10msble",
            "bgr0",
            "bgra",
            "rgb0",
            "rgba",
            "gbrp",
            "gbrp10msble",
        ]),
        "av1_nvenc" => Some(&["yuv420p", "nv12", "p010le", "yuv420p10le"]),

        "libx264" => Some(&[
            "yuv420p",
            "yuv422p",
            "yuv444p",
            "yuv420p10le",
            "yuv422p10le",
            "yuv444p10le",
        ]),
        "libx265" | "libaom-av1" => Some(&[
            "yuv420p",
            "yuv422p",
            "yuv444p",
            "yuv420p10le",
            "yuv422p10le",
            "yuv444p10le",
            "yuv420p12le",
            "yuv422p12le",
            "yuv444p12le",
        ]),
        "libsvtav1" => Some(&["yuv420p", "yuv420p10le"]),
        "libvpx-vp9" | "libvpx" => Some(&[
            "yuv420p",
            "yuv422p",
            "yuv444p",
            "yuv420p10le",
            "yuv422p10le",
            "yuv444p10le",
        ]),
        "prores_ks" => Some(&["yuv422p10le", "yuv444p10le"]),
        "libwebp" => Some(&["yuv420p", "bgra"]),

        enc if enc.ends_with("_vaapi") => Some(&[]),

        "h264_qsv" => Some(&["nv12"]),
        "hevc_qsv" => Some(&[
            "nv12",
            "p010le",
            "yuyv422",
            "y210le",
            "bgra",
            "x2rgb10le",
            "vuyx",
            "xv30le",
        ]),
        "av1_qsv" => Some(&["nv12", "p010le"]),
        "vp9_qsv" => Some(&["nv12", "p010le", "vuyx", "xv30le"]),
        enc if enc.ends_with("_amf") => Some(&[
            "nv12",
            "yuv420p",
            "p010le",
            "bgr0",
            "rgb0",
            "bgra",
            "argb",
            "rgba",
            "x2bgr10le",
            "rgbaf16le",
        ]),
        enc if enc.ends_with("_videotoolbox") => Some(&["nv12", "p010le", "uyvy422", "bgra"]),

        _ => None,
    }
}

// formats to try for `input_pix_fmt`, in order, excluding `attempted`, avoiding any format with more bit depth than the source and any format `encoder` does not accept
pub fn retry_pix_fmts(
    encoder: &str,
    input_pix_fmt: &str,
    attempted: &[String],
) -> Vec<&'static str> {
    let source_depth = pix_fmt_bit_depth(input_pix_fmt).max(8);
    let encoder_depth = max_bit_depth(encoder).unwrap_or(u8::MAX);
    let max_depth = source_depth.min(encoder_depth);
    let accepted = encoder_formats(encoder);

    PIX_FMT_LADDER
        .iter()
        .copied()
        .filter(|candidate| pix_fmt_bit_depth(candidate) <= max_depth)
        .filter(|candidate| accepted.is_none_or(|list| list.contains(candidate)))
        .filter(|candidate| !attempted.iter().any(|a| a == candidate))
        .collect()
}
