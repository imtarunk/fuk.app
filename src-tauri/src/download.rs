use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::config::{self, AppConfig, LlmModelId, WhisperModelId};
use crate::hardware;

#[derive(Debug, Clone, Serialize)]
pub struct ModelFileStatus {
    pub id: String,
    pub filename: String,
    pub present: bool,
    pub size_bytes: Option<u64>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub whisper: Vec<ModelFileStatus>,
    pub llm: Vec<ModelFileStatus>,
    pub ready_for_fast: bool,
    pub ready_for_polish: bool,
    pub machine_tier: String,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgressPayload {
    pub model: String,
    pub downloaded: u64,
    pub total: u64,
}

pub fn get_model_status(config: &AppConfig) -> ModelStatus {
    let whisper: Vec<ModelFileStatus> = WhisperModelId::all()
        .into_iter()
        .map(|m| file_status(m.as_id(), m.filename(), m == config.whisper_model))
        .collect();
    let llm: Vec<ModelFileStatus> = LlmModelId::all()
        .into_iter()
        .map(|m| file_status(m.as_id(), m.filename(), m == config.llm_model))
        .collect();
    let ready_for_fast = config.whisper_model.path().is_file();
    let ready_for_polish = ready_for_fast && config.llm_model.path().is_file();
    ModelStatus {
        whisper,
        llm,
        ready_for_fast,
        ready_for_polish,
        machine_tier: hardware::machine_tier().as_id().to_string(),
    }
}

fn file_status(id: &str, filename: &str, required: bool) -> ModelFileStatus {
    let path = config::models_dir().join(filename);
    let (present, size_bytes) = match fs::metadata(&path) {
        Ok(meta) if meta.is_file() => (true, Some(meta.len())),
        _ => (false, None),
    };
    ModelFileStatus {
        id: id.to_string(),
        filename: filename.to_string(),
        present,
        size_bytes,
        required,
    }
}

pub fn whisper_missing(config: &AppConfig) -> bool {
    !config.whisper_model.path().is_file()
}

struct DownloadSpec {
    id: String,
    filename: String,
    url: String,
}

fn spec_whisper(id: WhisperModelId) -> DownloadSpec {
    let filename = id.filename().to_string();
    DownloadSpec {
        id: id.as_id().to_string(),
        url: format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{filename}"
        ),
        filename,
    }
}

fn spec_llm(id: LlmModelId) -> DownloadSpec {
    match id {
        LlmModelId::SmolLm2_360m => DownloadSpec {
            id: id.as_id().to_string(),
            filename: id.filename().to_string(),
            url: "https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/main/SmolLM2-360M-Instruct-Q4_K_M.gguf"
                .to_string(),
        },
        LlmModelId::Qwen3_06b => DownloadSpec {
            id: id.as_id().to_string(),
            filename: id.filename().to_string(),
            url: "https://huggingface.co/bartowski/Qwen_Qwen3-0.6B-GGUF/resolve/main/Qwen_Qwen3-0.6B-Q4_K_M.gguf"
                .to_string(),
        },
    }
}

fn needed_downloads(config: &AppConfig) -> Vec<DownloadSpec> {
    let specs = vec![
        spec_whisper(config.whisper_model),
        spec_llm(config.llm_model),
    ];
    specs
        .into_iter()
        .filter(|s| !config::models_dir().join(&s.filename).is_file())
        .collect()
}

pub fn pending_downloads(config: &AppConfig) -> bool {
    !needed_downloads(config).is_empty()
}

pub async fn start_model_download(app: &AppHandle, config: &AppConfig) -> Result<()> {
    config::ensure_dirs()?;
    let needed = needed_downloads(config);
    if needed.is_empty() {
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .user_agent("dictate/0.1")
        .timeout(Duration::from_secs(60 * 60))
        .connect_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(16))
        .build()
        .context("http client")?;

    for spec in needed {
        download_one(app, &client, &spec).await?;
    }
    Ok(())
}

async fn download_one(app: &AppHandle, client: &reqwest::Client, spec: &DownloadSpec) -> Result<()> {
    let dest: PathBuf = config::models_dir().join(&spec.filename);
    let partial = PathBuf::from(format!("{}.partial", dest.display()));
    let result = download_one_inner(app, client, spec, &dest, &partial).await;
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

async fn download_one_inner(
    app: &AppHandle,
    client: &reqwest::Client,
    spec: &DownloadSpec,
    dest: &PathBuf,
    partial: &PathBuf,
) -> Result<()> {

    let response = client
        .get(&spec.url)
        .send()
        .await
        .with_context(|| format!("request {}", spec.url))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "download {} failed: HTTP {}",
            spec.id,
            response.status()
        ));
    }
    let total = response.content_length().unwrap_or(0);
    let mut stream = response.bytes_stream();
    let mut file = File::create(&partial).with_context(|| format!("create {}", partial.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_emit = 0u64;

    let _ = app.emit(
        "download-progress",
        DownloadProgressPayload {
            model: spec.id.clone(),
            downloaded: 0,
            total,
        },
    );

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream")?;
        file.write_all(&chunk).context("write model chunk")?;
        downloaded += chunk.len() as u64;
        if downloaded.saturating_sub(last_emit) >= 256 * 1024 || downloaded == total {
            last_emit = downloaded;
            let _ = app.emit(
                "download-progress",
                DownloadProgressPayload {
                    model: spec.id.clone(),
                    downloaded,
                    total,
                },
            );
        }
    }
    file.flush().ok();
    drop(file);
    fs::rename(&partial, &dest).with_context(|| {
        format!(
            "rename {} -> {}",
            partial.display(),
            dest.display()
        )
    })?;
    let _ = app.emit(
        "download-progress",
        DownloadProgressPayload {
            model: spec.id.clone(),
            downloaded,
            total: if total == 0 { downloaded } else { total },
        },
    );
    Ok(())
}
