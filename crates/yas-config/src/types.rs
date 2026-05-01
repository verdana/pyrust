use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct GeneralConfig {
    pub mode: InputMode,
    pub switch_key: String,
    pub candidate_key: CandidateKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Zh,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKey {
    Space,
    Number,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub fuzzy_pinyin: bool,
    pub enable_bigram: bool,
    pub personal_learning: bool,
}

#[derive(Debug, Clone)]
pub struct DictConfig {
    pub base_dict_path: String,
    pub user_dict_path: String,
    pub bigram_data_path: String,
    pub auto_learn: bool,
    pub max_user_dict_size: usize,
}

#[derive(Debug, Clone)]
pub struct UiConfig {
    pub font_size: u32,
    pub font_family: String,
    pub theme: Theme,
    pub max_candidates: usize,
    /// Deprecated: layout is always horizontal. This field is ignored.
    pub vertical: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    Auto,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            mode: InputMode::Zh,
            switch_key: "Shift".into(),
            candidate_key: CandidateKey::Space,
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            fuzzy_pinyin: false,
            enable_bigram: true,
            personal_learning: true,
        }
    }
}

impl Default for DictConfig {
    fn default() -> Self {
        Self {
            base_dict_path: "base.dict".into(),
            user_dict_path: "user.db".into(),
            bigram_data_path: "bigram.dat".into(),
            auto_learn: true,
            max_user_dict_size: 100_000,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            font_size: 18,
            font_family: String::new(),
            theme: Theme::Auto,
            max_candidates: 5,
            vertical: false,
        }
    }
}

impl Default for InputMode {
    fn default() -> Self {
        Self::Zh
    }
}

impl Default for CandidateKey {
    fn default() -> Self {
        Self::Space
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::Auto
    }
}

// --- Raw TOML types for serde ---

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct RawConfig {
    #[serde(default)]
    pub general: RawGeneralConfig,
    #[serde(default)]
    pub engine: RawEngineConfig,
    #[serde(default)]
    pub dict: RawDictConfig,
    #[serde(default)]
    pub ui: RawUiConfig,
}

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct RawGeneralConfig {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub switch_key: String,
    #[serde(default)]
    pub candidate_key: String,
}

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct RawEngineConfig {
    #[serde(default)]
    pub fuzzy_pinyin: bool,
    #[serde(default)]
    pub enable_bigram: bool,
    #[serde(default)]
    pub personal_learning: bool,
}

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct RawDictConfig {
    #[serde(default)]
    pub base_dict_path: String,
    #[serde(default)]
    pub user_dict_path: String,
    #[serde(default)]
    pub bigram_data_path: String,
    #[serde(default)]
    pub auto_learn: bool,
    #[serde(default)]
    pub max_user_dict_size: usize,
}

#[derive(Deserialize, Serialize, Default)]
pub(crate) struct RawUiConfig {
    #[serde(default)]
    pub font_size: u32,
    #[serde(default)]
    pub font_family: String,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub max_candidates: usize,
    #[serde(default)]
    pub vertical: bool,
}

// --- Conversions ---

impl From<RawGeneralConfig> for GeneralConfig {
    fn from(raw: RawGeneralConfig) -> Self {
        Self {
            mode: match raw.mode.as_str() {
                "en" => InputMode::En,
                _ => InputMode::Zh,
            },
            switch_key: if raw.switch_key.is_empty() {
                "Shift".into()
            } else {
                raw.switch_key
            },
            candidate_key: match raw.candidate_key.as_str() {
                "Number" => CandidateKey::Number,
                _ => CandidateKey::Space,
            },
        }
    }
}

impl From<&GeneralConfig> for RawGeneralConfig {
    fn from(cfg: &GeneralConfig) -> Self {
        Self {
            mode: match cfg.mode {
                InputMode::Zh => "zh".into(),
                InputMode::En => "en".into(),
            },
            switch_key: cfg.switch_key.clone(),
            candidate_key: match cfg.candidate_key {
                CandidateKey::Space => "Space".into(),
                CandidateKey::Number => "Number".into(),
            },
        }
    }
}

impl From<RawEngineConfig> for EngineConfig {
    fn from(raw: RawEngineConfig) -> Self {
        Self {
            fuzzy_pinyin: raw.fuzzy_pinyin,
            enable_bigram: raw.enable_bigram,
            personal_learning: raw.personal_learning,
        }
    }
}

impl From<&EngineConfig> for RawEngineConfig {
    fn from(cfg: &EngineConfig) -> Self {
        Self {
            fuzzy_pinyin: cfg.fuzzy_pinyin,
            enable_bigram: cfg.enable_bigram,
            personal_learning: cfg.personal_learning,
        }
    }
}

impl From<RawDictConfig> for DictConfig {
    fn from(raw: RawDictConfig) -> Self {
        Self {
            base_dict_path: if raw.base_dict_path.is_empty() {
                "base.dict".into()
            } else {
                raw.base_dict_path
            },
            user_dict_path: if raw.user_dict_path.is_empty() {
                "user.db".into()
            } else {
                raw.user_dict_path
            },
            bigram_data_path: if raw.bigram_data_path.is_empty() {
                "bigram.dat".into()
            } else {
                raw.bigram_data_path
            },
            auto_learn: raw.auto_learn,
            max_user_dict_size: if raw.max_user_dict_size == 0 {
                100_000
            } else {
                raw.max_user_dict_size
            },
        }
    }
}

impl From<&DictConfig> for RawDictConfig {
    fn from(cfg: &DictConfig) -> Self {
        Self {
            base_dict_path: cfg.base_dict_path.clone(),
            user_dict_path: cfg.user_dict_path.clone(),
            bigram_data_path: cfg.bigram_data_path.clone(),
            auto_learn: cfg.auto_learn,
            max_user_dict_size: cfg.max_user_dict_size,
        }
    }
}

impl From<RawUiConfig> for UiConfig {
    fn from(raw: RawUiConfig) -> Self {
        Self {
            font_size: if raw.font_size == 0 { 18 } else { raw.font_size },
            font_family: raw.font_family,
            theme: match raw.theme.as_str() {
                "light" => Theme::Light,
                "dark" => Theme::Dark,
                _ => Theme::Auto,
            },
            max_candidates: if raw.max_candidates == 0 {
                5
            } else {
                raw.max_candidates
            },
            vertical: raw.vertical,
        }
    }
}

impl From<&UiConfig> for RawUiConfig {
    fn from(cfg: &UiConfig) -> Self {
        Self {
            font_size: cfg.font_size,
            font_family: cfg.font_family.clone(),
            theme: match cfg.theme {
                Theme::Light => "light".into(),
                Theme::Dark => "dark".into(),
                Theme::Auto => "auto".into(),
            },
            max_candidates: cfg.max_candidates,
            vertical: cfg.vertical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_config_defaults() {
        let c = UiConfig::default();
        assert_eq!(c.font_size, 18);
        assert_eq!(c.font_family, "");
        assert_eq!(c.theme, Theme::Auto);
        assert_eq!(c.max_candidates, 5);
        assert!(!c.vertical);
    }

    #[test]
    fn theme_from_string() {
        let raw = RawUiConfig { theme: "light".into(), ..Default::default() };
        assert_eq!(UiConfig::from(raw).theme, Theme::Light);

        let raw = RawUiConfig { theme: "dark".into(), ..Default::default() };
        assert_eq!(UiConfig::from(raw).theme, Theme::Dark);

        let raw = RawUiConfig { theme: "unknown".into(), ..Default::default() };
        assert_eq!(UiConfig::from(raw).theme, Theme::Auto);
    }

    #[test]
    fn ui_config_zero_defaults() {
        let raw = RawUiConfig { font_size: 0, max_candidates: 0, ..Default::default() };
        let cfg = UiConfig::from(raw);
        assert_eq!(cfg.font_size, 18);
        assert_eq!(cfg.max_candidates, 5);
    }

    #[test]
    fn ui_config_roundtrip() {
        let cfg = UiConfig {
            font_size: 24,
            font_family: "SimSun".into(),
            theme: Theme::Dark,
            max_candidates: 9,
            vertical: false,
        };
        let raw = RawUiConfig::from(&cfg);
        let roundtripped = UiConfig::from(raw);
        assert_eq!(roundtripped.font_size, 24);
        assert_eq!(roundtripped.font_family, "SimSun");
        assert_eq!(roundtripped.theme, Theme::Dark);
        assert_eq!(roundtripped.max_candidates, 9);
    }

    #[test]
    fn toml_parse_ui_section() {
        let toml_str = r#"
[ui]
font_size = 22
font_family = "Fira Code"
theme = "dark"
max_candidates = 7
"#;
        let raw: RawConfig = toml::from_str(toml_str).unwrap();
        let cfg = UiConfig::from(raw.ui);
        assert_eq!(cfg.font_size, 22);
        assert_eq!(cfg.font_family, "Fira Code");
        assert_eq!(cfg.theme, Theme::Dark);
        assert_eq!(cfg.max_candidates, 7);
    }
}
