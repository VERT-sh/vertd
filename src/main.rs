mod converter;
mod http;
mod state;

use std::{env, process::exit, time::Duration};

use converter::gpu::{get_gpu, ConverterGPU};
use dotenv::dotenv;
use env_logger::Env;
use http::start_http;
use log::{error, info, warn};
use tokio::fs;

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
        _ => Err(anyhow::anyhow!(
            "{}. Valid options: amd, intel, nvidia, apple",
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

    match &gpu {
        Ok(gpu) => info!(
            "detected a{} {} GPU -- if this isn't your vendor, open an issue.",
            match gpu {
                converter::gpu::ConverterGPU::AMD => "n",
                converter::gpu::ConverterGPU::Apple => "n",
                converter::gpu::ConverterGPU::Intel => "n",
                _ => "",
            },
            gpu
        ),
        Err(e) => {
            error!("failed to get GPU vendor: {}", e);
            error!("vertd will still work, but it's going to be incredibly slow. be warned!");
        }
    }

    if let Ok(gpu) = gpu {
        let mut app_state = state::APP_STATE.lock().await;
        app_state.gpu = Some(gpu);
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

    start_http().await?;
    Ok(())
}
