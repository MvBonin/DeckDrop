use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub player_name: String,
    pub games_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player_name: "Player".to_string(),
            games_path: PathBuf::from("~/Games"),
        }
    }
}

impl Config {
    /// Lädt die Konfiguration aus der Datei oder gibt Standardwerte zurück
    pub fn load() -> Self {
        let config_path = Self::config_path();
        
        if let Some(path) = config_path {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<Config>(&content) {
                            Ok(config) => {
                                // Expandiere ~ im Pfad
                                let mut config = config;
                                config.games_path = Self::expand_path(&config.games_path);
                                return config;
                            }
                            Err(e) => {
                                eprintln!("Fehler beim Parsen der Konfiguration: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Fehler beim Lesen der Konfiguration: {}", e);
                    }
                }
            }
        }
        
        // Fallback zu Standardwerten
        Self::default()
    }

    /// Speichert die Konfiguration in eine Datei
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::config_path()
            .ok_or("Konnte Konfigurationspfad nicht bestimmen")?;
        
        // Erstelle das Verzeichnis falls es nicht existiert
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Normalisiere den Pfad für die Speicherung (konvertiere zu ~ wenn im Home-Verzeichnis)
        let mut config_to_save = self.clone();
        config_to_save.games_path = Self::normalize_path_for_save(&config_to_save.games_path);
        
        // Speichere die Konfiguration
        let json = serde_json::to_string_pretty(&config_to_save)?;
        fs::write(&config_path, json)?;
        
        Ok(())
    }

    /// Gibt den Pfad zur Konfigurationsdatei zurück
    fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
            .map(|dirs| dirs.config_dir().join("config.json"))
    }

    /// Expandiert ~ im Pfad zu einem vollständigen Pfad
    fn expand_path(path: &Path) -> PathBuf {
        let path_str = path.to_string_lossy();
        if path_str.starts_with("~/") {
            if let Some(home) = directories::BaseDirs::new() {
                return home.home_dir().join(&path_str[2..]);
            }
        }
        path.to_path_buf()
    }

    /// Normalisiert einen Pfad für die Speicherung (konvertiert zu ~ wenn im Home-Verzeichnis)
    fn normalize_path_for_save(path: &Path) -> PathBuf {
        if let Some(home) = directories::BaseDirs::new() {
            if let Ok(relative) = path.strip_prefix(home.home_dir()) {
                if relative.as_os_str().is_empty() {
                    return PathBuf::from("~");
                }
                return PathBuf::from("~").join(relative);
            }
        }
        path.to_path_buf()
    }
}

