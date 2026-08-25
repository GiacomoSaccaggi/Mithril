use anyhow::{anyhow, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;

use crate::engine::model_catalog::{find_model, MODELS};

pub async fn run(model: &str, list: bool) -> Result<()> {
    if list {
        println!("Available models:\n");
        for m in MODELS {
            println!("  {:15} — {}", m.id, m.display_name);
            println!("               {}", m.download_url);
            println!();
        }
        return Ok(());
    }

    let model_info = find_model(model)
        .ok_or_else(|| anyhow!("Unknown model: {model}\nRun: mithril download-model --list"))?;

    let model_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".mithril/models");

    fs::create_dir_all(&model_dir)?;

    let dest = model_dir.join(model_info.file_name);

    if dest.exists() {
        println!("✅ Already downloaded: {}", dest.display());
        return Ok(());
    }

    println!("⬇️  Downloading: {}", model_info.display_name);
    println!("   From: {}", model_info.download_url);
    println!("   To:   {}", dest.display());

    let client = reqwest::Client::builder()
        .user_agent("Mithril/0.1")
        .build()?;

    let response = client.get(model_info.download_url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!("Download failed: HTTP {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let tmp_dest = dest.with_extension("gguf.tmp");
    let mut file = fs::File::create(&tmp_dest)?;

    use std::io::Write;
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| anyhow!("Stream error: {}", e))?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }

    pb.finish_with_message("Download complete");

    // Atomic rename from .tmp to final path
    fs::rename(&tmp_dest, &dest)?;

    println!("\n✅ Downloaded: {}", dest.display());
    Ok(())
}

/// Download a model without any terminal output — safe to call from HTTP background tasks.
/// Uses the same download logic as `run()` but without progress bars or println.
pub async fn run_headless(model: &str) -> anyhow::Result<()> {
    let model_info = crate::engine::model_catalog::find_model(model)
        .ok_or_else(|| anyhow::anyhow!("Unknown model: {model}"))?;

    let model_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".mithril/models");
    std::fs::create_dir_all(&model_dir)?;

    let dest = model_dir.join(model_info.file_name);
    if dest.exists() {
        return Ok(());
    }

    let client = reqwest::Client::builder().user_agent("Mithril/0.1").build()?;
    let response = client.get(model_info.download_url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Download failed: HTTP {}", response.status());
    }

    let tmp_dest = dest.with_extension("gguf.tmp");
    let mut file = std::fs::File::create(&tmp_dest)?;
    use std::io::Write;
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| anyhow::anyhow!("Stream error: {}", e))?;
        file.write_all(&chunk)?;
    }
    std::fs::rename(&tmp_dest, &dest)?;
    Ok(())
}
