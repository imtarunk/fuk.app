use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppMode {
    Fast,
    Polish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppUiState {
    Idle,
    Recording,
    Processing,
    Downloading,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhisperModelId {
    #[serde(rename = "tiny.en")]
    TinyEn,
    #[serde(rename = "base.en")]
    BaseEn,
    #[serde(rename = "small.en")]
    SmallEn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmModelId {
    /// ChatML SmolLM2 360M Instruct Q4_K_M. Used on lower-spec machines.
    #[serde(rename = "smollm2-360m", alias = "llama-3.2-1b")]
    SmolLm2_360m,
    /// ChatML Qwen3 0.6B Q4_K_M. Used on higher-spec machines.
    #[serde(rename = "qwen3-0.6b", alias = "qwen2.5-1.5b")]
    Qwen3_06b,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InsertMode {
    Paste,
    ClipboardOnly,
}

/// Bumped when stored settings need a one-time rewrite. See [`migrate`].
const SETTINGS_VERSION: u32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub hotkey: String,
    pub mode: AppMode,
    pub whisper_model: WhisperModelId,
    pub llm_model: LlmModelId,
    pub input_device: Option<String>,
    pub insert_mode: InsertMode,
    pub first_run_complete: bool,
    pub overlay_x: Option<i32>,
    pub overlay_y: Option<i32>,
    /// Absent in files written before migrations existed, and those are exactly
    /// the ones that need migrating — so this must not inherit the container
    /// default, which is the current version.
    #[serde(default = "unmigrated")]
    pub settings_version: u32,
}

fn unmigrated() -> u32 {
    0
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            mode: AppMode::Polish,
            whisper_model: WhisperModelId::BaseEn,
            llm_model: recommended_llm(),
            input_device: None,
            insert_mode: default_insert_mode(),
            first_run_complete: false,
            overlay_x: None,
            overlay_y: None,
            settings_version: SETTINGS_VERSION,
        }
    }
}

impl WhisperModelId {
    pub fn as_id(self) -> &'static str {
        match self {
            Self::TinyEn => "tiny.en",
            Self::BaseEn => "base.en",
            Self::SmallEn => "small.en",
        }
    }

    pub fn filename(self) -> &'static str {
        match self {
            Self::TinyEn => "ggml-tiny.en.bin",
            Self::BaseEn => "ggml-base.en.bin",
            Self::SmallEn => "ggml-small.en.bin",
        }
    }

    pub fn path(self) -> PathBuf {
        models_dir().join(self.filename())
    }

    pub fn all() -> [Self; 3] {
        [Self::TinyEn, Self::BaseEn, Self::SmallEn]
    }
}

impl LlmModelId {
    pub fn as_id(self) -> &'static str {
        match self {
            Self::SmolLm2_360m => "smollm2-360m",
            Self::Qwen3_06b => "qwen3-0.6b",
        }
    }

    pub fn filename(self) -> &'static str {
        match self {
            Self::SmolLm2_360m => "SmolLM2-360M-Instruct-Q4_K_M.gguf",
            Self::Qwen3_06b => "Qwen_Qwen3-0.6B-Q4_K_M.gguf",
        }
    }

    pub fn path(self) -> PathBuf {
        models_dir().join(self.filename())
    }

    pub fn all() -> [Self; 2] {
        [Self::SmolLm2_360m, Self::Qwen3_06b]
    }
}

pub fn recommended_llm() -> LlmModelId {
    match crate::hardware::machine_tier() {
        crate::hardware::MachineTier::Lower => LlmModelId::SmolLm2_360m,
        crate::hardware::MachineTier::Higher => LlmModelId::Qwen3_06b,
    }
}

/// Re-pick the polish model from current hardware. Returns whether it changed.
pub fn apply_hardware_llm(cfg: &mut AppConfig) -> bool {
    let next = recommended_llm();
    if cfg.llm_model == next {
        return false;
    }
    log::info!(
        "auto-select polish model {} ({})",
        next.as_id(),
        crate::hardware::machine_tier().as_id()
    );
    cfg.llm_model = next;
    true
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Dictate")
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Dictate")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn ensure_dirs() -> Result<()> {
    fs::create_dir_all(config_dir()).context("create config dir")?;
    fs::create_dir_all(data_dir()).context("create data dir")?;
    fs::create_dir_all(models_dir()).context("create models dir")?;
    Ok(())
}

pub fn load() -> AppConfig {
    if let Err(e) = ensure_dirs() {
        log::warn!("failed to create Dictate dirs: {e}");
    }
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(raw) => match toml::from_str::<AppConfig>(&raw) {
            Ok(mut cfg) => {
                let migrated = migrate(&mut cfg);
                let picked = apply_hardware_llm(&mut cfg);
                if migrated || picked {
                    if let Err(e) = save(&cfg) {
                        log::warn!("failed to write migrated config: {e}");
                    }
                }
                cfg
            }
            Err(e) => {
                log::warn!("invalid config.toml ({e}), using defaults");
                AppConfig::default()
            }
        },
        Err(_) => {
            let cfg = AppConfig::default();
            if let Err(e) = save(&cfg) {
                log::warn!("failed to write default config: {e}");
            }
            cfg
        }
    }
}

pub fn save(config: &AppConfig) -> Result<()> {
    ensure_dirs()?;
    let raw = toml::to_string_pretty(config).context("serialize config")?;
    fs::write(config_path(), raw).context("write config.toml")?;
    Ok(())
}

pub fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE")
        .map(|v| v.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
        || std::env::var("WAYLAND_DISPLAY")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
}

/// Rewrites settings that an older build could store but that cannot work.
/// Returns whether anything changed.
fn migrate(cfg: &mut AppConfig) -> bool {
    if cfg.settings_version >= SETTINGS_VERSION {
        return false;
    }

    // macOS routes Fn/Globe to its own handler (emoji picker, dictation) before
    // any app sees it, so a Fn binding can never fire.
    if matches!(cfg.hotkey.as_str(), "Function" | "Fn") {
        log::info!("migrating unusable Fn hotkey to {}", default_hotkey());
        cfg.hotkey = default_hotkey();
    }
    // Clipboard-only exists as a Wayland fallback. Anywhere else it just makes
    // dictation look broken, because nothing is ever inserted.
    if cfg.insert_mode == InsertMode::ClipboardOnly && !is_wayland() {
        cfg.insert_mode = InsertMode::Paste;
    }

    // v3: Llama 3.2 / Qwen2.5 polish models are gone. Hardware pick happens
    // in apply_hardware_llm on every load, including this migration.
    if cfg.settings_version < 3 && cfg.mode == AppMode::Fast {
        cfg.mode = AppMode::Polish;
    }

    // v4: the idle ball moved to bottom-center; drop positions saved under the
    // old bottom-right default so everyone starts at the new spot.
    if cfg.settings_version < 4 {
        cfg.overlay_x = None;
        cfg.overlay_y = None;
    }

    cfg.settings_version = SETTINGS_VERSION;
    true
}

fn default_hotkey() -> String {
    if cfg!(target_os = "macos") {
        // A combo goes through RegisterEventHotKey, which needs no permission.
        // A lone modifier would need Accessibility before it ever works.
        "Alt+Space".to_string()
    } else {
        "ControlRight".to_string()
    }
}

fn default_insert_mode() -> InsertMode {
    if is_wayland() {
        InsertMode::ClipboardOnly
    } else {
        InsertMode::Paste
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> AppConfig {
        toml::from_str(raw).expect("valid config")
    }

    #[test]
    fn a_file_without_a_version_is_migrated() {
        let mut cfg = parse(
            r#"
            hotkey = "Function"
            insert_mode = "clipboard_only"
            first_run_complete = true
            "#,
        );
        assert_eq!(cfg.settings_version, 0);
        assert!(migrate(&mut cfg));
        assert_eq!(cfg.hotkey, default_hotkey());
        assert!(cfg.first_run_complete, "migration keeps unrelated settings");
        if !is_wayland() {
            assert_eq!(cfg.insert_mode, InsertMode::Paste);
        }
    }

    #[test]
    fn clipboard_only_on_macos_is_migrated_even_after_v1() {
        let mut cfg = parse(
            r#"
            hotkey = "ControlRight"
            insert_mode = "clipboard_only"
            settings_version = 1
            "#,
        );
        assert!(migrate(&mut cfg));
        assert_eq!(cfg.hotkey, "ControlRight");
        if !is_wayland() {
            assert_eq!(cfg.insert_mode, InsertMode::Paste);
        }
        assert_eq!(cfg.settings_version, SETTINGS_VERSION);
    }

    #[test]
    fn a_current_file_is_left_alone() {
        let mut cfg = parse(
            r#"
            hotkey = "ControlRight"
            insert_mode = "paste"
            settings_version = 4
            "#,
        );
        assert!(!migrate(&mut cfg));
        assert_eq!(cfg.insert_mode, InsertMode::Paste);
    }

    #[test]
    fn old_llama_id_maps_to_smollm() {
        let cfg: AppConfig = toml::from_str(r#"llm_model = "llama-3.2-1b""#).unwrap();
        assert_eq!(cfg.llm_model, LlmModelId::SmolLm2_360m);
    }
}
