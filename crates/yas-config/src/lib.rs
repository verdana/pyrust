mod types;
pub use types::*;

use std::path::PathBuf;

pub struct Config {
    pub general: GeneralConfig,
    pub engine: EngineConfig,
    pub dict: DictConfig,
    pub ui: UiConfig,
    path: PathBuf,
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let raw: RawConfig = toml::from_str(&content).unwrap_or_default();
        Self {
            general: raw.general.into(),
            engine: raw.engine.into(),
            dict: raw.dict.into(),
            ui: raw.ui.into(),
            path,
        }
    }

    pub fn save(&self) {
        let raw = RawConfig {
            general: RawGeneralConfig::from(&self.general),
            engine: RawEngineConfig::from(&self.engine),
            dict: RawDictConfig::from(&self.dict),
            ui: RawUiConfig::from(&self.ui),
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = toml::to_string_pretty(&raw).unwrap_or_default();
        let _ = std::fs::write(&self.path, content);
    }
}

fn config_path() -> PathBuf {
    if let Some(base) = directories::BaseDirs::new() {
        let mut path = base.config_dir().to_path_buf();
        path.push("inputd");
        path.push("config.toml");
        path
    } else {
        PathBuf::from("config.toml")
    }
}
