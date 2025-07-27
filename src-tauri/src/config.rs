use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tokio::sync::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckDropConfig {
    pub player_name: String,
    pub games_folder: String,
    pub network: NetworkConfig,
    pub ui: UIConfig,
    pub metadata: MetadataConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub discovery_enabled: bool,
    pub auto_connect: bool,
    pub max_peers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIConfig {
    pub theme: String,
    pub language: String,
    pub notifications_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataConfig {
    pub cache_enabled: bool,
    pub cache_size_mb: u64,
    pub auto_refresh: bool,
}

impl Default for DeckDropConfig {
    fn default() -> Self {
        Self {
            player_name: "DeckDrop_User".to_string(),
            games_folder: "~/Games/DeckDrop".to_string(),
            network: NetworkConfig::default(),
            ui: UIConfig::default(),
            metadata: MetadataConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            discovery_enabled: true,
            auto_connect: true,
            max_peers: 50,
        }
    }
}

impl Default for UIConfig {
    fn default() -> Self {
        Self {
            theme: "light".to_string(),
            language: "en".to_string(),
            notifications_enabled: true,
        }
    }
}

impl Default for MetadataConfig {
    fn default() -> Self {
        Self {
            cache_enabled: true,
            cache_size_mb: 100,
            auto_refresh: true,
        }
    }
}

#[derive(Clone)]
pub struct ConfigManager {
    config: Arc<RwLock<DeckDropConfig>>,
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = Self::get_config_dir()?;
        let config_path = config_dir.join("deckdrop.toml");
        
        if !config_path.exists() {
            return Err("Configuration file does not exist".into());
        }
        
        let config = match fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => config,
                Err(_) => {
                    println!("Failed to parse config file, using defaults");
                    DeckDropConfig::default()
                }
            },
            Err(_) => {
                println!("Failed to read config file, using defaults");
                DeckDropConfig::default()
            }
        };

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
        })
    }

    pub fn create_initial_config(player_name: String, games_folder: String) -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = Self::get_config_dir()?;
        let config_path = config_dir.join("deckdrop.toml");
        
        println!("Creating initial config at: {:?}", config_path);
        
        // Create default config with provided values
        let mut config = DeckDropConfig::default();
        config.player_name = player_name;
        config.games_folder = games_folder;
        
        let config_manager = Self {
            config: Arc::new(RwLock::new(config)),
            config_path,
        };
        
        // Save the config immediately - use async version
        let config_manager_clone = config_manager.clone();
        tokio::spawn(async move {
            match config_manager_clone.save_config().await {
                Ok(_) => println!("Initial config saved successfully"),
                Err(e) => eprintln!("Failed to save initial config: {}", e),
            }
        });
        
        Ok(config_manager)
    }

    pub async fn get_config(&self) -> DeckDropConfig {
        self.config.read().await.clone()
    }

    pub async fn update_config(&self, new_config: DeckDropConfig) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut config = self.config.write().await;
            *config = new_config;
        }
        
        self.save_config().await
    }

    pub async fn update_player_name(&self, name: String) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = self.config.write().await;
        config.player_name = name;
        drop(config);
        
        self.save_config().await
    }

    pub async fn update_games_folder(&self, folder: String) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = self.config.write().await;
        config.games_folder = folder;
        drop(config);
        
        self.save_config().await
    }

    pub async fn update_theme(&self, theme: String) -> Result<(), Box<dyn std::error::Error>> {
        let mut config = self.config.write().await;
        config.ui.theme = theme;
        drop(config);
        
        self.save_config().await
    }

    async fn save_config(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config = self.config.read().await;
        let content = toml::to_string_pretty(&*config)?;
        
        println!("Saving config to: {:?}", self.config_path);
        
        // Ensure config directory exists
        if let Some(parent) = self.config_path.parent() {
            println!("Creating parent directory: {:?}", parent);
            fs::create_dir_all(parent)?;
        }
        
        fs::write(&self.config_path, content)?;
        println!("Config saved successfully");
        Ok(())
    }

    pub fn get_config_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Use XDG base directories for system-agnostic config location
        // This works properly with Flatpak and other sandboxed environments
        let config_dir = dirs::config_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
            .ok_or("Could not determine config directory")?;
        
        let deckdrop_config_dir = config_dir.join("deckdrop");
        println!("Config directory: {:?}", deckdrop_config_dir);
        
        Ok(deckdrop_config_dir)
    }

    pub fn get_metadata_dir(&self) -> PathBuf {
        // Use XDG data directory for metadata storage
        let data_dir = dirs::data_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("share")))
            .unwrap_or_else(|| PathBuf::from("."));
        data_dir.join("deckdrop").join("metadata")
    }

    pub fn get_cache_dir(&self) -> PathBuf {
        // Use XDG cache directory for cache storage
        let cache_dir = dirs::cache_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join(".cache")))
            .unwrap_or_else(|| PathBuf::from("."));
        cache_dir.join("deckdrop")
    }
}

// Tauri commands
#[tauri::command]
pub async fn get_config() -> Result<DeckDropConfig, String> {
    let config_dir = ConfigManager::get_config_dir()
        .map_err(|e| format!("Failed to get config directory: {}", e))?;
    let config_path = config_dir.join("deckdrop.toml");
    
    if !config_path.exists() {
        return Err("No configuration file found. First-time setup required.".to_string());
    }
    
    let config_manager = ConfigManager::new()
        .map_err(|e| format!("Failed to create config manager: {}", e))?;
    
    Ok(config_manager.get_config().await)
}

#[tauri::command]
pub async fn check_config_exists() -> Result<bool, String> {
    let config_dir = ConfigManager::get_config_dir()
        .map_err(|e| format!("Failed to get config directory: {}", e))?;
    let config_path = config_dir.join("deckdrop.toml");
    
    Ok(config_path.exists())
}

#[tauri::command]
pub async fn update_player_name(name: String) -> Result<(), String> {
    let config_manager = ConfigManager::new()
        .map_err(|e| format!("Failed to create config manager: {}", e))?;
    
    config_manager.update_player_name(name).await
        .map_err(|e| format!("Failed to update player name: {}", e))
}

#[tauri::command]
pub async fn update_games_folder(folder: String) -> Result<(), String> {
    let config_manager = ConfigManager::new()
        .map_err(|e| format!("Failed to create config manager: {}", e))?;
    
    config_manager.update_games_folder(folder).await
        .map_err(|e| format!("Failed to update games folder: {}", e))
}

#[tauri::command]
pub async fn update_theme(theme: String) -> Result<(), String> {
    let config_manager = ConfigManager::new()
        .map_err(|e| format!("Failed to create config manager: {}", e))?;
    
    config_manager.update_theme(theme).await
        .map_err(|e| format!("Failed to update theme: {}", e))
}

#[tauri::command]
pub async fn save_initial_config(player_name: String, games_folder: String) -> Result<(), String> {
    let _config_manager = ConfigManager::create_initial_config(player_name, games_folder)
        .map_err(|e| format!("Failed to create initial config manager: {}", e))?;
    
    // Wait a bit for the async save to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_config_creation() {
        let config = DeckDropConfig::default();
        assert_eq!(config.player_name, "DeckDrop_User");
        assert_eq!(config.games_folder, "~/Games/DeckDrop");
        assert_eq!(config.ui.theme, "light");
    }

    #[tokio::test]
    async fn test_config_serialization() {
        let config = DeckDropConfig::default();
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: DeckDropConfig = toml::from_str(&serialized).unwrap();
        
        assert_eq!(config.player_name, deserialized.player_name);
        assert_eq!(config.games_folder, deserialized.games_folder);
    }

    #[tokio::test]
    async fn test_config_update() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");
        
        // Create a temporary config manager
        let config_manager = ConfigManager {
            config: Arc::new(RwLock::new(DeckDropConfig::default())),
            config_path: config_path.clone(),
        };

        // Test player name update
        config_manager.update_player_name("TestUser".to_string()).await.unwrap();
        let updated_config = config_manager.get_config().await;
        assert_eq!(updated_config.player_name, "TestUser");

        // Test games folder update
        config_manager.update_games_folder("/custom/path".to_string()).await.unwrap();
        let updated_config = config_manager.get_config().await;
        assert_eq!(updated_config.games_folder, "/custom/path");
    }
} 