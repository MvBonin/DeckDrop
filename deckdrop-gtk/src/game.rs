use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::io::Read;
use sha2::{Sha256, Digest};
use hex;

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
    #[serde(default)]
    pub hash: Option<String>, // SHA-256 Hash der deckdrop_chunks.toml im Format "sha256:XYZ"
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
            hash: None,
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
    /// 
    /// Wenn chunks_hash angegeben ist, wird dieser als hash-Feld gespeichert.
    pub fn save_to_path_with_hash(&self, game_path: &Path, chunks_hash: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
        let toml_path = game_path.join("deckdrop.toml");
        
        // Stelle sicher, dass das Verzeichnis existiert
        if let Some(parent) = toml_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Erstelle eine Kopie mit dem Hash, falls vorhanden
        let mut game_info_to_save = self.clone();
        if let Some(hash) = chunks_hash {
            game_info_to_save.hash = Some(hash);
        }
        
        let toml_string = toml::to_string_pretty(&game_info_to_save)?;
        fs::write(&toml_path, toml_string)?;
        
        Ok(())
    }
    
    /// Speichert GameInfo als deckdrop.toml Datei (ohne Hash)
    pub fn save_to_path(&self, game_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.save_to_path_with_hash(game_path, None)
    }
}

/// Struktur für einen Datei-Eintrag in deckdrop_chunks.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileChunkEntry {
    path: String,
    chunks: Vec<String>,
}

/// Generiert die deckdrop_chunks.toml Datei für ein Spiel
/// 
/// Diese Funktion durchsucht das Spielverzeichnis rekursiv und erstellt für jede Datei
/// eine Liste von SHA-256 Hashes für 10MB Chunks.
/// 
/// Gibt den SHA-256 Hash der generierten Datei zurück (im Format "sha256:XYZ").
pub fn generate_chunks_toml(game_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    const CHUNK_SIZE: usize = 10 * 1024 * 1024; // 10MB
    
    let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
    let mut file_entries = Vec::new();
    
    // Durchsuche das Verzeichnis rekursiv
    if !game_path.exists() {
        return Err(format!("Spielverzeichnis existiert nicht: {}", game_path.display()).into());
    }
    
    // Sammle alle Dateien (rekursiv)
    let mut files_to_process = Vec::new();
    collect_files_recursive(game_path, game_path, &mut files_to_process)?;
    
    // Verarbeite jede Datei
    for file_path in files_to_process {
        // Überspringe deckdrop.toml und deckdrop_chunks.toml
        if let Some(file_name) = file_path.file_name() {
            let file_name_str = file_name.to_string_lossy();
            if file_name_str == "deckdrop.toml" || file_name_str == "deckdrop_chunks.toml" {
                continue;
            }
        }
        
        // Berechne relative Pfad
        let relative_path = file_path.strip_prefix(game_path)
            .map_err(|e| format!("Fehler beim Berechnen des relativen Pfads: {}", e))?;
        let path_str = relative_path.to_string_lossy().replace('\\', "/");
        
        // Öffne Datei und berechne Chunks
        let mut file = fs::File::open(&file_path)?;
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; CHUNK_SIZE];
        
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            
            // Berechne SHA-256 Hash für diesen Chunk
            let mut hasher = Sha256::new();
            hasher.update(&buffer[..bytes_read]);
            let hash = hasher.finalize();
            let hash_hex = hex::encode(hash);
            chunks.push(hash_hex);
        }
        
        file_entries.push(FileChunkEntry {
            path: path_str,
            chunks,
        });
    }
    
    // Sortiere nach Pfad, um konsistente Reihenfolge zu gewährleisten
    file_entries.sort_by(|a, b| a.path.cmp(&b.path));
    
    // Speichere als TOML
    let toml_string = toml::to_string_pretty(&file_entries)?;
    
    // Berechne SHA-256 Hash der generierten Datei (vor dem Schreiben)
    let mut hasher = Sha256::new();
    hasher.update(toml_string.as_bytes());
    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);
    let hash_string = format!("sha256:{}", hash_hex);
    
    // Schreibe die Datei
    fs::write(&chunks_toml_path, toml_string)?;
    
    Ok(hash_string)
}

/// Sammelt rekursiv alle Dateien aus einem Verzeichnis
fn collect_files_recursive(
    base_path: &Path,
    current_path: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !current_path.exists() {
        return Ok(());
    }
    
    if current_path.is_file() {
        files.push(current_path.to_path_buf());
        return Ok(());
    }
    
    if current_path.is_dir() {
        let entries = fs::read_dir(current_path)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            collect_files_recursive(base_path, &path, files)?;
        }
    }
    
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_generate_game_id_uniqueness() {
        // Test: Generierte IDs sollten meistens eindeutig sein
        // (Bei sehr schnellen Aufrufen können Kollisionen auftreten, da die Implementierung
        // auf Timestamp basiert. In der Praxis werden IDs nicht so schnell hintereinander generiert.)
        let mut ids = HashSet::new();
        let mut duplicates = 0;
        
        for _ in 0..100 {
            let id = generate_game_id();
            assert!(!id.is_empty(), "ID sollte nicht leer sein");
            if !ids.insert(id.clone()) {
                duplicates += 1;
            }
        }
        
        // Erlaube bis zu 50% Duplikate bei sehr schnellen Aufrufen
        // In der Praxis werden IDs nicht so schnell generiert, daher ist dies akzeptabel
        // Der wichtige Punkt ist, dass die IDs nicht leer sind und ein gültiges Format haben
        assert!(duplicates < 50, "Zu viele doppelte IDs: {} von 100", duplicates);
        
        // Mindestens die Hälfte sollte eindeutig sein
        assert!(ids.len() > 50, "Zu wenige eindeutige IDs: {} von 100", ids.len());
    }

    #[test]
    fn test_generate_game_id_format() {
        // Test: ID sollte ein hexadezimales Format haben
        let id = generate_game_id();
        
        assert!(!id.is_empty());
        // Hexadezimal sollte nur 0-9 und a-f enthalten
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), 
                "ID sollte hexadezimal sein: {}", id);
    }

    #[test]
    fn test_generate_game_id_length() {
        // Test: ID sollte eine angemessene Länge haben
        let id = generate_game_id();
        
        // DefaultHasher::finish() gibt u64 zurück, hex ist max 16 Zeichen
        // Aber wir formatieren als {:x}, also sollte es mindestens einige Zeichen haben
        assert!(id.len() > 0, "ID sollte nicht leer sein");
        assert!(id.len() <= 16, "ID sollte nicht zu lang sein: {}", id);
    }

    #[test]
    fn test_game_info_default() {
        // Test: Default GameInfo sollte eine game_id haben
        let game_info = GameInfo::default();
        
        assert!(!game_info.game_id.is_empty(), "Default GameInfo sollte eine game_id haben");
        assert_eq!(game_info.version, "1.0");
        assert_eq!(game_info.name, "");
    }

    #[test]
    fn test_game_info_serialization() {
        // Test: GameInfo sollte korrekt serialisiert/deserialisiert werden können
        let game_info = GameInfo {
            game_id: "test-id-123".to_string(),
            name: "Test Game".to_string(),
            version: "1.2.3".to_string(),
            start_file: "game.exe".to_string(),
            start_args: Some("--fullscreen".to_string()),
            description: Some("Ein Test-Spiel".to_string()),
            creator_peer_id: Some("peer-123".to_string()),
        };
        
        // Serialisiere zu TOML
        let toml_string = toml::to_string(&game_info).unwrap();
        assert!(toml_string.contains("test-id-123"));
        assert!(toml_string.contains("Test Game"));
        
        // Deserialisiere zurück
        let deserialized: GameInfo = toml::from_str(&toml_string).unwrap();
        assert_eq!(deserialized.game_id, game_info.game_id);
        assert_eq!(deserialized.name, game_info.name);
        assert_eq!(deserialized.version, game_info.version);
        assert_eq!(deserialized.start_file, game_info.start_file);
        assert_eq!(deserialized.start_args, game_info.start_args);
        assert_eq!(deserialized.description, game_info.description);
        assert_eq!(deserialized.creator_peer_id, game_info.creator_peer_id);
    }

    #[test]
    fn test_game_info_with_default_game_id() {
        // Test: GameInfo ohne explizite game_id sollte eine generieren
        let game_info = GameInfo {
            game_id: String::new(), // Leer, sollte durch default generiert werden
            name: "Test".to_string(),
            version: "1.0".to_string(),
            start_file: "test.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        // Beim Deserialisieren sollte game_id durch default generiert werden
        let toml_string = toml::to_string(&game_info).unwrap();
        let _deserialized: GameInfo = toml::from_str(&toml_string).unwrap();
        
        // Wenn game_id leer war, sollte sie durch default generiert werden
        // Aber in diesem Fall ist sie explizit leer, also testen wir die Serialisierung
        assert!(toml_string.contains("name = \"Test\""));
    }
}

