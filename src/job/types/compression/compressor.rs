use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::Command,
    sync::mpsc,
};

use crate::job::gpu::get_gpu;

use super::CompressionJob;

pub async fn first_pass(
    job: &CompressionJob,
    target_kb: u64,
) -> anyhow::Result<mpsc::Receiver<ProgressUpdate>> {
    let (tx, rx) = mpsc::channel(64);
    let gpu = get_gpu().await?;
    let null = if env::consts::OS == "windows" {
        "nul".to_string()
    } else {
        "/dev/null".to_string()
    };

    let encoder = job.format.codec(&gpu).await;

    let mut process = Command::new("ffmpeg")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .args(&[
            "-y",
            "-i",
            &format!("./input/{}.{}", job.id, job.format),
            "-c:v",
            &encoder,
            "-b:v",
            &format!("{}k", target_kb),
            "-pass",
            "1",
            "-hide_banner",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-passlogfile",
            &format!("./output/{}", job.id),
            "-an",
            "-f",
            "null",
            &null,
        ])
        .spawn()?;

    let tx_arc = Arc::new(tx);
    let tx = Arc::clone(&tx_arc);

    let stderr = process
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to take stderr"))?;

    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::error!("{}", line);
            tx.send(ProgressUpdate::Error(line)).await.unwrap();
        }
    });

    let stdout = process
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to take stdout"))?;

    let reader = BufReader::new(stdout);
    let tx = Arc::clone(&tx_arc);

    tokio::spawn(async move {
        let mut lines = reader.lines();
        while let Ok(Some(out)) = lines.next_line().await {
            let mut map = HashMap::new();
            for line in out.split("\n") {
                if let Some((k, v)) = line.split_once("=") {
                    map.insert(k.trim(), v.trim());
                }
            }

            let mut reports = Vec::new();

            if let Some(frame) = map.get("frame").and_then(|s| s.parse().ok()) {
                reports.push(ProgressUpdate::Frame(frame));
            }

            for report in reports {
                if tx.send(report).await.is_err() {
                    break;
                }
            }
        }
    });

    // let mut child = command.spawn()?;
    // let status = child.wait().await?;

    // let mut command = Command::new("ffmpeg");
    // let command = command.args(&[
    //     "-i",
    //     &format!("./input/{}.{}", self.id, self.format),
    //     "-c:v",
    //     &encoder,
    //     "-b:v",
    //     &format!("{}k", size_kb),
    //     "-pass",
    //     "2",
    //     "-passlogfile",
    //     &format!("./output/{}", self.id),
    //     "-c:a",
    //     "aac",
    //     "-b:a",
    //     "128k",
    //     &format!("./output/{}.{}", self.id, self.format),
    // ]);

    // log::info!("second pass is running");
    // let mut child = command.spawn()?;
    // let status = child.wait().await?;

    Ok(rx)
}

pub async fn second_pass(
    job: &CompressionJob,
    target_kb: u64,
) -> anyhow::Result<mpsc::Receiver<ProgressUpdate>> {
    let (tx, rx) = mpsc::channel(64);
    let gpu = get_gpu().await?;
    let null = if env::consts::OS == "windows" {
        "nul".to_string()
    } else {
        "/dev/null".to_string()
    };

    let encoder = job.format.codec(&gpu).await;

    let mut process = Command::new("ffmpeg")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .args(&[
            "-y",
            "-i",
            &format!("./input/{}.{}", job.id, job.format),
            "-c:v",
            &encoder,
            "-b:v",
            &format!("{}k", target_kb),
            "-pass",
            "2",
            "-hide_banner",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-passlogfile",
            &format!("./output/{}", job.id),
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            &format!("./output/{}.{}", job.id, job.format),
        ])
        .spawn()?;

    let tx_arc = Arc::new(tx);
    let tx = Arc::clone(&tx_arc);

    let stderr = process
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to take stderr"))?;

    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::error!("{}", line);
            tx.send(ProgressUpdate::Error(line)).await.unwrap();
        }
    });

    let stdout = process
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to take stdout"))?;

    let reader = BufReader::new(stdout);
    let tx = Arc::clone(&tx_arc);

    tokio::spawn(async move {
        let mut lines = reader.lines();
        while let Ok(Some(out)) = lines.next_line().await {
            let mut map = HashMap::new();
            for line in out.split("\n") {
                if let Some((k, v)) = line.split_once("=") {
                    map.insert(k.trim(), v.trim().to_string());
                }
            }

            if let Some(frame) = map.get("frame").and_then(|s| s.parse().ok()) {
                if tx.send(ProgressUpdate::Frame(frame)).await.is_err() {
                    break;
                }
            }
        }
    });

    Ok(rx)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum ProgressUpdate {
    #[serde(rename = "frame", rename_all = "camelCase")]
    Frame(u64),
    #[serde(rename = "error", rename_all = "camelCase")]
    Error(String),
}
