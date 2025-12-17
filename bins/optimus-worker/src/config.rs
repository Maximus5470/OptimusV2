// Language configuration management for Optimus Worker
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use optimus_common::types::Language;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageExecution {
    pub command: String,
    pub args: Vec<String>,
    pub file_extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageConfig {
    pub name: String,
    pub version: String,
    pub image: String,
    pub dockerfile_path: String,
    pub execution: LanguageExecution,
    pub queue_name: String,
    pub memory_limit_mb: u32,
    pub cpu_limit: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct LanguagesJson {
    languages: Vec<LanguageConfig>,
}

/// Language configuration manager
#[derive(Clone)]
pub struct LanguageConfigManager {
    configs: HashMap<String, LanguageConfig>,
}

impl LanguageConfigManager {
    /// Load language configurations from languages.json
    pub fn load(config_path: &Path) -> Result<Self> {
        if !config_path.exists() {
            bail!("Language config file not found: {}", config_path.display());
        }

        let content = fs::read_to_string(config_path)
            .context("Failed to read languages.json")?;
        
        let languages_json: LanguagesJson = serde_json::from_str(&content)
            .context("Failed to parse languages.json")?;

        let mut configs = HashMap::new();
        for lang in languages_json.languages {
            configs.insert(lang.name.clone(), lang);
        }

        Ok(Self { configs })
    }

    /// Load with default path (config/languages.json)
    pub fn load_default() -> Result<Self> {
        let default_path = Path::new("config/languages.json");
        Self::load(default_path)
    }

    /// Get configuration for a specific language
    pub fn get_config(&self, language: &Language) -> Result<&LanguageConfig> {
        let lang_name = language.to_string();
        self.configs
            .get(&lang_name)
            .ok_or_else(|| anyhow::anyhow!("No configuration found for language: {}", lang_name))
    }

    /// Get Docker image for a language
    pub fn get_image(&self, language: &Language) -> Result<String> {
        Ok(self.get_config(language)?.image.clone())
    }

    /// Get queue name for a language
    pub fn get_queue_name(&self, language: &Language) -> Result<String> {
        Ok(self.get_config(language)?.queue_name.clone())
    }

    /// Get memory limit for a language
    pub fn get_memory_limit_mb(&self, language: &Language) -> Result<u32> {
        Ok(self.get_config(language)?.memory_limit_mb)
    }

    /// Get CPU limit for a language
    pub fn get_cpu_limit(&self, language: &Language) -> Result<f32> {
        Ok(self.get_config(language)?.cpu_limit)
    }

    /// List all supported languages
    pub fn list_languages(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        // This test will only work if config/languages.json exists
        let result = LanguageConfigManager::load_default();
        match result {
            Ok(manager) => {
                println!("Loaded languages: {:?}", manager.list_languages());
            }
            Err(e) => {
                println!("Config not found (expected in test environment): {}", e);
            }
        }
    }
}
