use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Struktur für Spiel-Informationen
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    #[serde(default = "generate_game_id")]
    pub game_id: String,
    pub name: String,
    pub version: String,
    pub start_file: String,
    #[serde(default)]
    pub start_args: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub creator_peer_id: Option<String>,
}

/// Generiert eine eindeutige Spiel-ID
pub fn generate_game_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let mut hasher = DefaultHasher::new();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    
    // Verwende auch den Speicherort des Hashers als zusätzliche Zufälligkeit
    let addr = &hasher as *const _ as usize;
    
    timestamp.hash(&mut hasher);
    addr.hash(&mut hasher);
    
    format!("{:x}", hasher.finish())
}

impl Default for GameInfo {
    fn default() -> Self {
        Self {
            game_id: generate_game_id(),
            name: String::new(),
            version: "1.0".to_string(),
            start_file: String::new(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        }
    }
}

impl GameInfo {
    /// Lädt GameInfo aus einer deckdrop.toml Datei
    pub fn load_from_path(game_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let toml_path = game_path.join("deckdrop.toml");
        
        if !toml_path.exists() {
            return Err(format!("deckdrop.toml nicht gefunden in: {}", game_path.display()).into());
        }
        
        let content = fs::read_to_string(&toml_path)?;
        let mut game_info: GameInfo = toml::from_str(&content)?;
        
        // Stelle sicher, dass eine game_id vorhanden ist (für alte Spiele ohne ID)
        if game_info.game_id.is_empty() {
            game_info.game_id = generate_game_id();
            // Speichere die aktualisierte TOML mit der neuen ID
            let _ = game_info.save_to_path(game_path);
        }
        
        Ok(game_info)
    }
    
    /// Speichert GameInfo als deckdrop.toml Datei
    pub fn save_to_path(&self, game_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let toml_path = game_path.join("deckdrop.toml");
        
        // Stelle sicher, dass das Verzeichnis existiert
        if let Some(parent) = toml_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let toml_string = toml::to_string_pretty(self)?;
        fs::write(&toml_path, toml_string)?;
        
        Ok(())
    }
}

/// Prüft, ob im angegebenen Spielpfad bereits eine deckdrop.toml existiert
pub fn check_game_config_exists(game_path: &Path) -> bool {
    let toml_path = game_path.join("deckdrop.toml");
    toml_path.exists()
}

/// Lädt alle Spiele aus einem Spiele-Verzeichnis
pub fn load_games_from_directory(games_dir: &Path) -> Vec<(PathBuf, GameInfo)> {
    let mut games = Vec::new();
    
    if !games_dir.exists() {
        return games;
    }
    
    let entries = match fs::read_dir(games_dir) {
        Ok(entries) => entries,
        Err(_) => return games,
    };
    
    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.is_dir() {
                if check_game_config_exists(&path) {
                    if let Ok(game_info) = GameInfo::load_from_path(&path) {
                        games.push((path, game_info));
                    }
                }
            }
        }
    }
    
    games
}

