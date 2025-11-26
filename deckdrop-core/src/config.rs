use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

// Statische Variable für den aktuellen Peer-ID-Unterordner
static PEER_ID_SUBDIR: OnceLock<Option<String>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub player_name: String,
    #[serde(rename = "games_path", alias = "download_path")]
    pub download_path: PathBuf,
    #[serde(default)]
    pub peer_id: Option<String>,
    #[serde(default)]
    pub game_paths: Vec<PathBuf>,
    #[serde(default = "default_max_concurrent_chunks")]
    pub max_concurrent_chunks: usize,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

fn default_max_concurrent_chunks() -> usize {
    15  // Erhöht von 5 auf 15 für bessere Parallelisierung mit 10MB Chunks
}

fn default_max_connections() -> usize {
    50  // Maximale Anzahl gleichzeitiger Peer-Verbindungen
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player_name: "Player".to_string(),
            download_path: PathBuf::from("~/Games"),
            peer_id: None,
            game_paths: Vec::new(),
            max_concurrent_chunks: 15,
            max_connections: 50,
        }
    }
}

impl Config {
    /// Setzt den Peer-ID-Unterordner für die Konfiguration
    /// Wird verwendet, wenn --random-id gesetzt ist
    pub fn set_peer_id_subdir(peer_id: &str) {
        PEER_ID_SUBDIR.set(Some(peer_id.to_string())).ok();
    }
    
    /// Gibt den aktuellen Peer-ID-Unterordner zurück
    pub fn get_peer_id_subdir() -> Option<String> {
        PEER_ID_SUBDIR.get().and_then(|s| s.clone())
    }
    
    /// Lädt die Konfiguration aus der Datei oder gibt Standardwerte zurück
    /// Generiert KEINE Peer-ID automatisch - das muss explizit über generate_and_save_peer_id() gemacht werden
    /// 
    /// WICHTIG: Wenn kein Peer-ID-Unterordner gesetzt ist (z.B. durch --random-id), wird NUR das Hauptverzeichnis verwendet.
    /// Es werden KEINE Unterordner durchsucht, um sicherzustellen, dass die normale Config immer verwendet wird.
    pub fn load() -> Self {
        // Versuche zuerst, die Peer-ID aus dem Hauptverzeichnis zu laden
        let main_keypair_path = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
            .map(|dirs| dirs.config_dir().join("peer_id.key"));
        
        // Wenn kein Peer-ID-Unterordner gesetzt ist, versuche die Keypair-Datei im Hauptverzeichnis zu finden
        // WICHTIG: Durchsuche KEINE Unterordner - verwende nur das Hauptverzeichnis
        if Self::get_peer_id_subdir().is_none() {
            if let Some(ref main_path) = main_keypair_path {
                if main_path.exists() {
                    if let Some(_peer_id) = Self::load_peer_id_from_keypair_file(main_path) {
                        // Keypair im Hauptverzeichnis gefunden - verwende Hauptverzeichnis
                        // Kein Unterordner wird gesetzt - bleibt im Hauptverzeichnis
                    }
                }
            }
            // KEINE Suche in Unterordnern - nur Hauptverzeichnis wird verwendet
        }
        // Wenn Peer-ID-Unterordner bereits gesetzt ist (z.B. durch --random-id), wird die Suche übersprungen
        
        let config_path = Self::config_path();
        
        let mut config = if let Some(path) = config_path {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        match serde_json::from_str::<Config>(&content) {
                            Ok(mut config) => {
                                // Expandiere ~ im Pfad
                                config.download_path = Self::expand_path(&config.download_path);
                                // Expandiere ~ in allen Spiel-Pfaden
                                config.game_paths = config.game_paths.iter()
                                    .map(|p| Self::expand_path(p))
                                    .collect();
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
    
    /// Lädt die Peer-ID aus einer spezifischen Keypair-Datei
    fn load_peer_id_from_keypair_file(keypair_path: &Path) -> Option<String> {
        use libp2p::identity;
        use libp2p::PeerId;
        
        if !keypair_path.exists() {
            return None;
        }
        
        match fs::read(keypair_path) {
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
    
    /// Prüft, ob bereits eine Peer-ID existiert (ohne eine zu generieren)
    pub fn has_peer_id() -> bool {
        // Prüfe zuerst im Hauptverzeichnis
        let main_keypair_path = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
            .map(|dirs| dirs.config_dir().join("peer_id.key"));
        
        if let Some(ref main_path) = main_keypair_path {
            if main_path.exists() {
                return true;
            }
        }
        
        // Wenn nicht im Hauptverzeichnis, durchsuche Unterordner
        if let Some(base_dir) = directories::ProjectDirs::from("com", "deckdrop", "deckdrop") {
            let config_dir = base_dir.config_dir();
            if let Ok(entries) = fs::read_dir(config_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let keypair_path = path.join("peer_id.key");
                        if keypair_path.exists() {
                            return true;
                        }
                    }
                }
            }
        }
        
        false
    }
    
    /// Generiert eine neue Peer-ID und speichert sie
    pub fn generate_and_save_peer_id(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(peer_id) = Self::load_or_generate_peer_id() {
            self.peer_id = Some(peer_id.clone());
            // Setze den Peer-ID-Unterordner, wenn noch nicht gesetzt
            if Self::get_peer_id_subdir().is_none() {
                Self::set_peer_id_subdir(&peer_id);
            }
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
        let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")?;
        let config_dir = base_dir.config_dir();
        
        // Wenn ein Peer-ID-Unterordner gesetzt ist, verwende diesen
        if let Some(subdir) = Self::get_peer_id_subdir() {
            Some(config_dir.join(&subdir).join("peer_id.key"))
        } else {
            Some(config_dir.join("peer_id.key"))
        }
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
        // Normalisiere alle Spiel-Pfade
        config_to_save.game_paths = config_to_save.game_paths.iter()
            .map(|p| Self::normalize_path_for_save(p))
            .collect();
        
        // Speichere die Konfiguration
        let json = serde_json::to_string_pretty(&config_to_save)?;
        fs::write(&config_path, json)?;
        
        Ok(())
    }
    
    /// Fügt einen Spiel-Pfad zur Konfiguration hinzu
    pub fn add_game_path(&mut self, game_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let normalized_path = Self::normalize_path_for_save(game_path);
        if !self.game_paths.contains(&normalized_path) {
            self.game_paths.push(normalized_path);
            self.save()?;
        }
        Ok(())
    }
    
    /// Entfernt einen Spiel-Pfad aus der Konfiguration
    pub fn remove_game_path(&mut self, game_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let normalized_path = Self::normalize_path_for_save(game_path);
        self.game_paths.retain(|p| p != &normalized_path);
        self.save()?;
        Ok(())
    }

    /// Gibt den Pfad zur Konfigurationsdatei zurück
    pub fn config_path() -> Option<PathBuf> {
        let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")?;
        let config_dir = base_dir.config_dir();
        
        // Wenn ein Peer-ID-Unterordner gesetzt ist, verwende diesen
        if let Some(subdir) = Self::get_peer_id_subdir() {
            Some(config_dir.join(&subdir).join("config.json"))
        } else {
            Some(config_dir.join("config.json"))
        }
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

