mod converter;
mod http;
mod state;

use std::{env, process::exit, time::Duration};

use converter::gpu::{get_gpu, ConverterGPU};
use dotenv::dotenv;
use env_logger::Env;
use http::start_http;
use log::{error, info, warn};
use once_cell::sync::Lazy;
use tokio::{fs, process::Command};

pub const INPUT_LIFETIME: Duration = Duration::from_secs(60 * 60);
pub const OUTPUT_LIFETIME: Duration = Duration::from_secs(60 * 60);

enum FFUtil {
    FFmpeg,
    FFprobe,
}

async fn ffutil_version(util: FFUtil) -> anyhow::Result<String> {
    let program = match util {
        FFUtil::FFmpeg => "ffmpeg",
        FFUtil::FFprobe => "ffprobe",
    };
    let output = tokio::process::Command::new(program)
        .arg("-version")
        .output()
        .await?;
    let version = String::from_utf8(output.stdout)?;
    // from "ffmpeg version 7.1 .... .. .. . ." get "7.1"
    let version = version.split_whitespace().nth(2).ok_or_else(|| {
        anyhow::anyhow!(
            "failed to get version from output (this is a bug in vertd! please report!)"
        )
    })?;

    Ok(version.to_string())
}

fn parse_gpu(gpu_str: &str) -> anyhow::Result<ConverterGPU> {
    match gpu_str.to_lowercase().as_str() {
        "amd" => Ok(ConverterGPU::AMD),
        "intel" => Ok(ConverterGPU::Intel),
        "nvidia" => Ok(ConverterGPU::NVIDIA),
        "apple" => Ok(ConverterGPU::Apple),
        "cpu" => Ok(ConverterGPU::CPU),
        _ => Err(anyhow::anyhow!(
            "{}. Valid options: amd, intel, nvidia, apple, cpu",
            gpu_str
        )),
    }
}

fn get_forced_gpu() -> Option<ConverterGPU> {
    // cli argument (-gpu <value>)
    let args: Vec<String> = env::args().collect();
    if let Some(gpu_arg_pos) = args.iter().position(|arg| arg == "-gpu" || arg == "--gpu") {
        if let Some(gpu_value) = args.get(gpu_arg_pos + 1) {
            match parse_gpu(gpu_value) {
                Ok(gpu) => {
                    info!("using GPU from command line argument: {}", gpu);
                    return Some(gpu);
                }
                Err(e) => {
                    warn!("invalid GPU specified in command line argument: {}", e);
                }
            }
        } else {
            warn!("GPU argument specified but no value provided");
        }
    }

    // environment variable
    if let Ok(gpu_env) = env::var("VERTD_FORCE_GPU") {
        match parse_gpu(&gpu_env) {
            Ok(gpu) => {
                info!(
                    "using GPU from environment variable VERTD_FORCE_GPU: {}",
                    gpu
                );
                return Some(gpu);
            }
            Err(e) => {
                warn!(
                    "invalid GPU specified in VERTD_FORCE_GPU environment variable: {}",
                    e
                );
            }
        }
    }

    None
}

fn get_vaapi_device_path() -> Option<String> {
    // cli argument (-vaapi-device <value>)
    let args: Vec<String> = env::args().collect();
    if let Some(vaapi_arg_pos) = args
        .iter()
        .position(|arg| arg == "-vaapi-device" || arg == "--vaapi-device")
    {
        if let Some(device_value) = args.get(vaapi_arg_pos + 1) {
            info!(
                "using VA-API device path from command line argument: {}",
                device_value
            );
            return Some(device_value.clone());
        } else {
            warn!("VA-API device path argument specified but no value provided");
        }
    }

    // environment variable
    if let Ok(device_path) = env::var("VERTD_VAAPI_DEVICE_PATH") {
        info!(
            "using VA-API device path from environment variable VERTD_VAAPI_DEVICE_PATH: {}",
            device_path
        );
        return Some(device_path);
    }

    None
}

pub static MAX_UPLOAD_BYTES: Lazy<Option<usize>> = Lazy::new(|| {
    match std::env::var("MAX_UPLOAD_BYTES") {
        Ok(value) => {
            let trimmed = value.trim();
            // unlimited if empty
            if trimmed.is_empty() {
                None
            } else {
                match trimmed.parse::<usize>() {
                    Ok(0) => None, // unlimited if set to 0
                    Ok(limit) => Some(limit),
                    Err(e) => {
                        warn!(
                        "invalid MAX_UPLOAD_BYTES value '{}': {}. falling back to no upload size limit",
                        trimmed,
                        e
                    );
                        None
                    }
                }
            }
        }
        Err(_) => None,
    }
});

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::Builder::from_env(Env::default().default_filter_or("vertd")).init();
    info!("starting vertd");
    let ffmpeg_version = match ffutil_version(FFUtil::FFmpeg).await {
        Ok(version) => version,
        Err(e) => {
            log::error!("failed to get ffmpeg version -- vertd requires ffmpeg to be set up on the path or next to the executable ({})", e);
            exit(1);
        }
    };

    let ffprobe_version = match ffutil_version(FFUtil::FFprobe).await {
        Ok(version) => version,
        Err(e) => {
            log::error!("failed to get ffprobe version -- vertd requires ffprobe to be set up on the path or next to the executable ({})", e);
            exit(1);
        }
    };

    info!(
        "working w/ ffmpeg {} and ffprobe {}",
        ffmpeg_version, ffprobe_version
    );

    // check if env var or cli arg is specified for gpu, if not fallback to auto-detection
    let gpu = match get_forced_gpu() {
        Some(forced_gpu) => Ok(forced_gpu),
        None => get_gpu().await,
    };

    // get VA-API device path from CLI or env var
    let vaapi_device_path = get_vaapi_device_path();

    match &gpu {
        Ok(gpu) => {
            if matches!(gpu, ConverterGPU::CPU) {
                info!("using CPU rendering (software encoding) -- this will be slower than GPU acceleration");
            } else {
                info!(
                    "detected a{} {} GPU -- if this isn't your vendor, open an issue.",
                    match gpu {
                        ConverterGPU::AMD => "n",
                        ConverterGPU::Apple => "n",
                        ConverterGPU::Intel => "n",
                        _ => "",
                    },
                    gpu
                );
            }

            #[cfg(target_os = "linux")]
            if matches!(gpu, ConverterGPU::AMD | ConverterGPU::Intel) {
                let device_path = vaapi_device_path
                    .as_deref()
                    .unwrap_or("/dev/dri/renderD128");
                info!("using VA-API device path: {}", device_path);
            }
        }
        Err(e) => {
            error!("failed to get GPU vendor: {}", e);
            warn!("falling back to CPU rendering (software encoding) -- this will be slower than GPU acceleration");
        }
    }

    // default to CPU if detection failed
    let gpu = gpu.unwrap_or(ConverterGPU::CPU);

    // check which accelerated codecs are actually supported by this GPU
    let accelerated_codecs = check_accelerated_codecs(gpu).await;

    {
        let mut app_state = state::APP_STATE.lock().await;
        app_state.gpu = Some(gpu);
        app_state.vaapi_device_path = vaapi_device_path;
        app_state.supported_accelerated_codecs = accelerated_codecs;
    }

    // remove input/ and output/ recursively if they exist -- we don't care if this fails tho
    let _ = fs::remove_dir_all("input").await;
    let _ = fs::remove_dir_all("output").await;

    // create input/ and output/ directories
    fs::create_dir("input").await?;
    fs::create_dir("output").await?;

    // also a permanent/ directory for kept files
    match fs::create_dir("permanent").await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }

    if let Some(limit) = *MAX_UPLOAD_BYTES {
        info!(
            "max upload size set to {} bytes ({} MB)",
            limit,
            limit / 1024 / 1024
        );
    } else {
        info!("no max upload size set - unlimited size allowed");
    }

    start_http().await?;
    Ok(())
}

// checks if the gpu supports accelerated encoding
// builds supported_accelerated_codecs in AppState to avoid unnecessary errors/conversions (see format.rs#accelerated_or_default_codec)
async fn check_accelerated_codecs(gpu: ConverterGPU) -> Vec<String> {
    let test_codecs = vec!["h264", "av1", "vp9", "vp8", "mpeg2"];
    let mut supported = Vec::new();
    let mut unsupported = Vec::new();

    if matches!(gpu, ConverterGPU::CPU) {
        info!("using CPU rendering, skipping accelerated codec checks");
        return supported;
    }

    info!("running accelerated codec checks");
    for codec in test_codecs {
        let encoder = match gpu.get_accelerated_codec(codec).await {
            Ok(enc) => enc,
            Err(_) => {
                unsupported.push(codec.to_string());
                continue;
            }
        };

        let process = Command::new("ffmpeg")
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=1280x720:rate=30",
                "-c:v",
                &encoder,
                "-f",
                "null",
                "-",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn();

        match process {
            Ok(child) => match child.wait_with_output().await {
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.contains("Error while opening encoder") && output.status.success() {
                        supported.push(codec.to_string());
                    } else {
                        unsupported.push(codec.to_string());
                    }
                }
                Err(e) => {
                    warn!(
                        "failed to wait on ffmpeg process for codec {}: {}",
                        codec, e
                    );
                }
            },
            Err(e) => {
                warn!("failed to execute ffmpeg for codec {}: {}", codec, e);
            }
        }
    }

    info!("supported accelerated codecs: {:?}", supported);
    info!("unsupported accelerated codecs: {:?}", unsupported);
    supported
}
