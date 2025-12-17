// Language configuration management
// Loads and validates languages from languages.json

use optimus_common::types::Language;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageConfig {
    pub name: String,
    pub version: String,
    pub image: String,
    pub dockerfile_path: String,
    pub execution: ExecutionConfig,
    pub queue_name: String,
    pub memory_limit_mb: u32,
    pub cpu_limit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub command: String,
    pub args: Vec<String>,
    pub file_extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LanguagesFile {
    languages: Vec<LanguageConfig>,
}

/// Registry of configured languages
/// This is the authoritative source for which languages are enabled
#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    enabled_languages: HashSet<Language>,
}

impl LanguageRegistry {
    /// Load language configuration from languages.json
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read languages.json: {}", e))?;
        
        let config: LanguagesFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse languages.json: {}", e))?;
        
        let mut enabled_languages = HashSet::new();
        
        for lang_config in &config.languages {
            match Language::from_str(&lang_config.name) {
                Some(lang) => {
                    enabled_languages.insert(lang);
                }
                None => {
                    return Err(format!(
                        "Unknown language '{}' in languages.json",
                        lang_config.name
                    ));
                }
            }
        }
        
        if enabled_languages.is_empty() {
            return Err("No languages configured in languages.json".to_string());
        }
        
        Ok(Self { enabled_languages })
    }
    
    /// Check if a language is enabled
    pub fn is_enabled(&self, language: Language) -> bool {
        self.enabled_languages.contains(&language)
    }
    
    /// Get all enabled languages
    pub fn enabled_languages(&self) -> Vec<Language> {
        self.enabled_languages.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_language_registry() {
        // This test assumes languages.json exists in config/
        let registry = LanguageRegistry::load_from_file("../../config/languages.json");
        assert!(registry.is_ok());
        
        if let Ok(reg) = registry {
            // Should have at least python
            assert!(reg.is_enabled(Language::Python));
        }
    }
}
