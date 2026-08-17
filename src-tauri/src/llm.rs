use std::num::NonZeroU32;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use once_cell::sync::OnceCell;

use crate::config::LlmModelId;

const SYSTEM_PROMPT: &str = "You clean up raw speech-to-text transcripts. Fix grammar, punctuation, and casing. Remove filler words and false starts. Do not add new information, do not change the meaning, do not answer questions in the text — only clean it up. Return only the cleaned text, nothing else.";

const N_CTX: u32 = 2048;
const MAX_NEW_TOKENS: usize = 512;

pub struct LlmEngine {
    model: LlamaModel,
    pub kind: LlmModelId,
}

fn backend() -> Result<&'static LlamaBackend> {
    static BACKEND: OnceCell<LlamaBackend> = OnceCell::new();
    BACKEND.get_or_try_init(|| LlamaBackend::init().map_err(|e| anyhow!("llama backend: {e}")))
}

impl LlmEngine {
    pub fn load(kind: LlmModelId) -> Result<Self> {
        let path = kind.path();
        if !path.exists() {
            return Err(anyhow!(
                "LLM model {} is not downloaded ({})",
                kind.as_id(),
                path.display()
            ));
        }
        load_from_path(kind, &path)
    }

    pub fn polish(&self, transcript: &str) -> Result<String> {
        if transcript.trim().is_empty() {
            return Ok(String::new());
        }
        let prompt = build_prompt(self.kind, transcript);
        let add_bos = match self.kind {
            LlmModelId::Llama32_1b => AddBos::Always,
            LlmModelId::Qwen25_15b => AddBos::Never,
        };
        let tokens = self
            .model
            .str_to_token(&prompt, add_bos)
            .context("tokenize polish prompt")?;
        if tokens.is_empty() {
            return Ok(String::new());
        }

        let n_threads = (num_cpus::get() as i32).clamp(1, 8);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_CTX)
            .with_n_threads(n_threads)
            .with_n_threads_batch(n_threads);
        let mut ctx = self
            .model
            .new_context(backend()?, ctx_params)
            .context("create llama context")?;

        let n_prompt = tokens.len();
        let max_prompt = (N_CTX as usize).saturating_sub(MAX_NEW_TOKENS).max(1);
        let tokens = if n_prompt > max_prompt {
            tokens[n_prompt - max_prompt..].to_vec()
        } else {
            tokens
        };
        let n_prompt = tokens.len();

        let mut batch = LlamaBatch::new(N_CTX as usize, 1);
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i == n_prompt - 1)
                .context("llama batch add prompt")?;
        }
        ctx.decode(&mut batch).context("decode prompt")?;

        let mut sampler = LlamaSampler::chain_simple([LlamaSampler::greedy()]);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut output = String::new();
        let mut pos = n_prompt as i32;

        for _ in 0..MAX_NEW_TOKENS {
            let idx = batch.n_tokens() - 1;
            let token = sampler.sample(&ctx, idx);
            sampler.accept(token);
            if self.model.is_eog_token(token) {
                break;
            }

            match self.model.token_to_piece(token, &mut decoder, false, None) {
                Ok(piece) => output.push_str(&piece),
                Err(e) => log::debug!("token_to_piece: {e}"),
            }

            batch.clear();
            batch
                .add(token, pos, &[0], true)
                .context("llama batch add token")?;
            ctx.decode(&mut batch).context("decode token")?;
            pos += 1;
        }

        Ok(sanitize_output(&output))
    }
}

fn load_from_path(kind: LlmModelId, path: &Path) -> Result<LlmEngine> {
    let backend = backend()?;
    let params = Default::default();
    let model = LlamaModel::load_from_file(backend, path, &params)
        .with_context(|| format!("load GGUF {}", path.display()))?;
    Ok(LlmEngine { model, kind })
}

fn build_prompt(kind: LlmModelId, user: &str) -> String {
    match kind {
        LlmModelId::Llama32_1b => format!(
            "<|start_header_id|>system<|end_header_id|>\n\n{SYSTEM_PROMPT}<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n{user}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n"
        ),
        LlmModelId::Qwen25_15b => format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
        ),
    }
}

fn sanitize_output(raw: &str) -> String {
    let mut s = raw.replace("<|eot_id|>", "");
    s = s.replace("<|im_end|>", "");
    s = s.replace("<|end_of_text|>", "");
    let s = s.trim();
    strip_wrapping_quotes(s).trim().to_string()
}

fn strip_wrapping_quotes(s: &str) -> &str {
    let t = s.trim();
    let mut chars = t.chars();
    let Some(first) = chars.next() else {
        return t;
    };
    let last = chars.next_back().unwrap_or(first);
    let pair = matches!(
        (first, last),
        ('"', '"') | ('\'', '\'') | ('\u{201c}', '\u{201d}') | ('\u{2018}', '\u{2019}')
    );
    if pair && t.len() >= first.len_utf8() + last.len_utf8() {
        &t[first.len_utf8()..t.len() - last.len_utf8()]
    } else {
        t
    }
}
