use anyhow::{anyhow, Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::config::WhisperModelId;

pub struct WhisperEngine {
    ctx: WhisperContext,
    pub model: WhisperModelId,
}

impl WhisperEngine {
    pub fn load(model: WhisperModelId) -> Result<Self> {
        install_logs_once();
        let path = model.path();
        if !path.exists() {
            return Err(anyhow!(
                "Whisper model {} is not downloaded ({})",
                model.as_id(),
                path.display()
            ));
        }
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("whisper model path is not valid UTF-8"))?;
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .with_context(|| format!("load whisper model {}", path.display()))?;
        Ok(Self { ctx, model })
    }

    /// Runs one throwaway inference so the first real utterance does not pay for
    /// lazily built Metal pipelines and scratch buffers.
    pub fn warm_up(&self) {
        let silence = vec![0.0f32; 16_000 / 2];
        if let Err(e) = self.transcribe(&silence) {
            log::debug!("whisper warm-up: {e}");
        }
    }

    pub fn transcribe(&self, pcm16k: &[f32]) -> Result<String> {
        if pcm16k.is_empty() {
            return Ok(String::new());
        }
        let mut state = self
            .ctx
            .create_state()
            .context("create whisper state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let lang = "en";
        params.set_language(Some(lang));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_no_timestamps(true);
        params.set_token_timestamps(false);
        // Temperature fallback silently re-decodes the whole clip up to six more
        // times when a heuristic is unhappy, which is the difference between a
        // snappy insert and a two second stall.
        params.set_temperature(0.0);
        params.set_temperature_inc(0.0);
        params.set_n_threads(inference_threads());

        state
            .full(params, pcm16k)
            .context("whisper inference")?;

        let mut text = String::new();
        for segment in state.as_iter() {
            match segment.to_str_lossy() {
                Ok(s) => text.push_str(&s),
                Err(e) => log::debug!("whisper segment: {e}"),
            }
        }
        Ok(text.trim().to_string())
    }
}

/// Physical cores only: whisper scales with real cores, and scheduling work onto
/// Apple silicon efficiency cores makes the slowest thread set the pace.
fn inference_threads() -> i32 {
    (num_cpus::get_physical() as i32).clamp(1, 8)
}

fn install_logs_once() {
    use std::sync::OnceLock;
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        whisper_rs::install_logging_hooks();
    });
}
