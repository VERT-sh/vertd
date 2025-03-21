mod http;
mod job;
mod state;

use std::{process::exit, time::Duration};

use dotenv::dotenv;
use env_logger::Env;
use http::start_http;
use job::gpu::{get_gpu, JobGPU};
use log::{error, info};
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

    let gpu = get_gpu().await;

    match gpu {
        Ok(gpu) => info!(
            "detected a{} {} GPU -- if this isn't your vendor, open an issue.",
            match gpu {
                JobGPU::AMD => "n",
                JobGPU::Apple => "n",
                _ => "",
            },
            gpu
        ),
        Err(e) => {
            error!("failed to get GPU vendor: {}", e);
            error!("vertd will still work, but it's going to be incredibly slow. be warned!");
        }
    }

    // remove input/ and output/ recursively if they exist -- we don't care if this fails tho
    let _ = fs::remove_dir_all("input").await;
    let _ = fs::remove_dir_all("output").await;

    // create input/ and output/ directories
    fs::create_dir("input").await?;
    fs::create_dir("output").await?;

    start_http().await?;
    Ok(())
}
