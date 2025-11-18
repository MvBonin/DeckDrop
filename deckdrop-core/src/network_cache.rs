//! Cache für Network-Games im Config-Verzeichnis

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::config::Config;

/// Struktur für gecachte Network-Game-Informationen
/// Enthält zusätzlich eine Liste der Peer-IDs, die dieses Spiel haben
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedNetworkGame {
    pub game_id: String,
    pub name: String,
    pub version: String,
    pub start_file: String,
    #[serde(default)]
    pub start_args: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub creator_peer_id: Option<String>,
    /// Liste der Peer-IDs, die dieses Spiel haben (für Online-Status)
    #[serde(default)]
    pub peer_ids: Vec<String>,
}

impl CachedNetworkGame {
    /// Konvertiert von NetworkGameInfo
    pub fn from_network_game_info(game_info: &deckdrop_network::network::games::NetworkGameInfo) -> Self {
        Self {
            game_id: game_info.game_id.clone(),
            name: game_info.name.clone(),
            version: game_info.version.clone(),
            start_file: game_info.start_file.clone(),
            start_args: game_info.start_args.clone(),
            description: game_info.description.clone(),
            creator_peer_id: game_info.creator_peer_id.clone(),
            peer_ids: Vec::new(),
        }
    }

    /// Konvertiert zu NetworkGameInfo
    pub fn to_network_game_info(&self) -> deckdrop_network::network::games::NetworkGameInfo {
        deckdrop_network::network::games::NetworkGameInfo {
            game_id: self.game_id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            start_file: self.start_file.clone(),
            start_args: self.start_args.clone(),
            description: self.description.clone(),
            creator_peer_id: self.creator_peer_id.clone(),
        }
    }
}

/// Gibt den Pfad zum Network-Games-Cache-Verzeichnis zurück
pub fn network_games_cache_dir() -> Option<PathBuf> {
    let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")?;
    let config_dir = base_dir.config_dir();
    
    // Wenn ein Peer-ID-Unterordner gesetzt ist, verwende diesen
    if let Some(subdir) = Config::get_peer_id_subdir() {
        Some(config_dir.join(&subdir).join("networkgames"))
    } else {
        Some(config_dir.join("networkgames"))
    }
}

/// Speichert ein Network-Game im Cache
pub fn save_network_game(game: &CachedNetworkGame) -> Result<(), Box<dyn std::error::Error>> {
    let cache_dir = network_games_cache_dir()
        .ok_or("Konnte Cache-Verzeichnis nicht bestimmen")?;
    
    // Erstelle das Verzeichnis falls es nicht existiert
    fs::create_dir_all(&cache_dir)?;
    
    // Speichere als gameid.toml
    let file_path = cache_dir.join(format!("{}.toml", game.game_id));
    let toml_string = toml::to_string_pretty(game)?;
    fs::write(&file_path, toml_string)?;
    
    Ok(())
}

/// Lädt ein Network-Game aus dem Cache
pub fn load_network_game(game_id: &str) -> Result<CachedNetworkGame, Box<dyn std::error::Error>> {
    let cache_dir = network_games_cache_dir()
        .ok_or("Konnte Cache-Verzeichnis nicht bestimmen")?;
    
    let file_path = cache_dir.join(format!("{}.toml", game_id));
    
    if !file_path.exists() {
        return Err(format!("Gecachtes Spiel nicht gefunden: {}", game_id).into());
    }
    
    let content = fs::read_to_string(&file_path)?;
    let game: CachedNetworkGame = toml::from_str(&content)?;
    
    Ok(game)
}

/// Lädt alle gecachten Network-Games
pub fn load_all_cached_network_games() -> Result<Vec<CachedNetworkGame>, Box<dyn std::error::Error>> {
    let cache_dir = network_games_cache_dir()
        .ok_or("Konnte Cache-Verzeichnis nicht bestimmen")?;
    
    if !cache_dir.exists() {
        return Ok(Vec::new());
    }
    
    let mut games = Vec::new();
    
    for entry in fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(game) = toml::from_str::<CachedNetworkGame>(&content) {
                    games.push(game);
                }
            }
        }
    }
    
    Ok(games)
}

/// Aktualisiert ein Network-Game im Cache (fügt Peer-ID hinzu oder aktualisiert)
pub fn update_network_game_peer(game_id: &str, peer_id: &str, game_info: &deckdrop_network::network::games::NetworkGameInfo) -> Result<(), Box<dyn std::error::Error>> {
    // Versuche zuerst, das gecachte Spiel zu laden
    let cached_game = match load_network_game(game_id) {
        Ok(mut game) => {
            // Aktualisiere Peer-IDs (füge hinzu, wenn nicht vorhanden)
            if !game.peer_ids.contains(&peer_id.to_string()) {
                game.peer_ids.push(peer_id.to_string());
            }
            // Aktualisiere auch die Spiel-Informationen (falls sich etwas geändert hat)
            game.name = game_info.name.clone();
            game.version = game_info.version.clone();
            game.start_file = game_info.start_file.clone();
            game.start_args = game_info.start_args.clone();
            game.description = game_info.description.clone();
            game.creator_peer_id = game_info.creator_peer_id.clone();
            game
        }
        Err(_) => {
            // Spiel existiert noch nicht im Cache, erstelle neues
            let mut new_game = CachedNetworkGame::from_network_game_info(game_info);
            new_game.peer_ids.push(peer_id.to_string());
            new_game
        }
    };
    
    save_network_game(&cached_game)?;
    
    Ok(())
}

/// Entfernt eine Peer-ID aus einem gecachten Network-Game
pub fn remove_peer_from_cached_game(game_id: &str, peer_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut cached_game = load_network_game(game_id)?;
    
    cached_game.peer_ids.retain(|id| id != peer_id);
    
    // Wenn keine Peers mehr vorhanden sind, behalte das Spiel trotzdem im Cache (für Offline-Anzeige)
    save_network_game(&cached_game)?;
    
    Ok(())
}

/// Löscht ein gecachtes Network-Game
pub fn delete_cached_network_game(game_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let cache_dir = network_games_cache_dir()
        .ok_or("Konnte Cache-Verzeichnis nicht bestimmen")?;
    
    let file_path = cache_dir.join(format!("{}.toml", game_id));
    
    if file_path.exists() {
        fs::remove_file(&file_path)?;
    }
    
    Ok(())
}

/// Löscht alle gecachten Network-Games
pub fn clear_all_cached_network_games() -> Result<(), Box<dyn std::error::Error>> {
    let cache_dir = network_games_cache_dir()
        .ok_or("Konnte Cache-Verzeichnis nicht bestimmen")?;
    
    if !cache_dir.exists() {
        return Ok(()); // Cache-Verzeichnis existiert nicht, nichts zu löschen
    }
    
    // Lösche alle .toml-Dateien im Cache-Verzeichnis
    for entry in fs::read_dir(&cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            if let Err(e) = fs::remove_file(&path) {
                eprintln!("Fehler beim Löschen von {:?}: {}", path, e);
            }
        }
    }
    
    Ok(())
}

