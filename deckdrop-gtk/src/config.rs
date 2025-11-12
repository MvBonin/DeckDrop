use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub player_name: String,
    #[serde(rename = "games_path", alias = "download_path")]
    pub download_path: PathBuf,
    #[serde(default)]
    pub peer_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player_name: "Player".to_string(),
            download_path: PathBuf::from("~/Games"),
            peer_id: None,
        }
    }
}

impl Config {
    /// Lädt die Konfiguration aus der Datei oder gibt Standardwerte zurück
    /// Generiert KEINE Peer-ID automatisch - das muss explizit über generate_and_save_peer_id() gemacht werden
    pub fn load() -> Self {
        let config_path = Self::config_path();
        
        let mut config = if let Some(path) = config_path {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<Config>(&content) {
                            Ok(mut config) => {
                                // Expandiere ~ im Pfad
                                config.download_path = Self::expand_path(&config.download_path);
                                config
                            }
                            Err(e) => {
                                eprintln!("Fehler beim Parsen der Konfiguration: {}", e);
                                Self::default()
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Fehler beim Lesen der Konfiguration: {}", e);
                        Self::default()
                    }
                }
            } else {
                Self::default()
            }
        } else {
            Self::default()
        };
        
        // Lade Peer-ID aus Keypair-Datei, falls vorhanden
        if config.peer_id.is_none() {
            if let Some(peer_id) = Self::load_peer_id_from_keypair() {
                config.peer_id = Some(peer_id);
            }
        }
        
        config
    }
    
    /// Prüft, ob bereits eine Peer-ID existiert (ohne eine zu generieren)
    pub fn has_peer_id() -> bool {
        Self::keypair_path()
            .map(|path| path.exists())
            .unwrap_or(false)
    }
    
    /// Generiert eine neue Peer-ID und speichert sie
    pub fn generate_and_save_peer_id(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(peer_id) = Self::load_or_generate_peer_id() {
            self.peer_id = Some(peer_id.clone());
            self.save()?;
            Ok(())
        } else {
            Err("Konnte Peer-ID nicht generieren".into())
        }
    }
    
    /// Lädt die Peer-ID aus der Keypair-Datei (ohne eine neue zu generieren)
    fn load_peer_id_from_keypair() -> Option<String> {
        use libp2p::identity;
        use libp2p::PeerId;
        
        let keypair_path = Self::keypair_path()?;
        
        if !keypair_path.exists() {
            return None;
        }
        
        match fs::read(&keypair_path) {
            Ok(bytes) => {
                match identity::Keypair::from_protobuf_encoding(&bytes) {
                    Ok(keypair) => {
                        let peer_id = PeerId::from(keypair.public());
                        Some(peer_id.to_string())
                    }
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    }
    
    /// Lädt die Keypair aus der Datei oder generiert eine neue
    /// Gibt die Peer-ID als String zurück
    /// WICHTIG: Diese Funktion generiert IMMER eine neue Keypair, wenn keine existiert
    /// oder wenn die Datei nicht gelesen werden kann
    fn load_or_generate_peer_id() -> Option<String> {
        use libp2p::identity;
        use libp2p::PeerId;
        
        let keypair_path = Self::keypair_path()?;
        
        // Versuche Keypair zu laden
        if keypair_path.exists() {
            match fs::read(&keypair_path) {
                Ok(bytes) => {
                    match identity::Keypair::from_protobuf_encoding(&bytes) {
                        Ok(keypair) => {
                            let peer_id = PeerId::from(keypair.public());
                            println!("Geladene Peer-ID: {}", peer_id);
                            eprintln!("Geladene Peer-ID: {}", peer_id);
                            return Some(peer_id.to_string());
                        }
                        Err(e) => {
                            eprintln!("Fehler beim Laden der Keypair: {}", e);
                            // Datei ist korrupt, lösche sie und generiere neue
                            let _ = fs::remove_file(&keypair_path);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Fehler beim Lesen der Keypair-Datei: {}", e);
                }
            }
        }
        
        // Generiere neue Keypair
        println!("Generiere neue Keypair...");
        eprintln!("Generiere neue Keypair...");
        let keypair = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(keypair.public());
        let peer_id_str = peer_id.to_string();
        
        // Speichere Keypair
        match keypair.to_protobuf_encoding() {
            Ok(bytes) => {
                if let Some(parent) = keypair_path.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        eprintln!("Fehler beim Erstellen des Verzeichnisses: {}", e);
                        return Some(peer_id_str);
                    }
                }
                if let Err(e) = fs::write(&keypair_path, bytes) {
                    eprintln!("Fehler beim Speichern der Keypair: {}", e);
                } else {
                    println!("Neue Peer-ID generiert und gespeichert: {}", peer_id);
                    eprintln!("Neue Peer-ID generiert und gespeichert: {}", peer_id);
                }
            }
            Err(e) => {
                eprintln!("Fehler beim Serialisieren der Keypair: {}", e);
            }
        }
        
        Some(peer_id_str)
    }
    
    /// Lädt die Keypair aus der Datei
    pub fn load_keypair() -> Option<libp2p::identity::Keypair> {
        use libp2p::identity;
        
        let keypair_path = Self::keypair_path()?;
        
        if !keypair_path.exists() {
            return None;
        }
        
        match fs::read(&keypair_path) {
            Ok(bytes) => {
                match identity::Keypair::from_protobuf_encoding(&bytes) {
                    Ok(keypair) => {
                        println!("Keypair erfolgreich geladen");
                        Some(keypair)
                    }
                    Err(e) => {
                        eprintln!("Fehler beim Laden der Keypair: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!("Fehler beim Lesen der Keypair-Datei: {}", e);
                None
            }
        }
    }
    
    /// Gibt den Pfad zur Keypair-Datei zurück
    pub fn keypair_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
            .map(|dirs| dirs.config_dir().join("peer_id.key"))
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
        config_to_save.download_path = Self::normalize_path_for_save(&config_to_save.download_path);
        
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

