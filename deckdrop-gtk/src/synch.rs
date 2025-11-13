use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use sha2::{Sha256, Digest};
use hex;

/// Manifest-Struktur für Download-Status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadManifest {
    pub game_id: String,
    pub game_name: String,
    pub game_path: String,
    pub chunks: HashMap<String, FileChunkInfo>,
    pub overall_status: DownloadStatus,
    pub progress: DownloadProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Paused,
    Complete,
    Error(String),
    Cancelled,
}

impl DownloadStatus {
    pub fn can_pause(&self) -> bool {
        matches!(self, DownloadStatus::Downloading)
    }
    
    pub fn can_resume(&self) -> bool {
        matches!(self, DownloadStatus::Paused)
    }
    
    pub fn can_cancel(&self) -> bool {
        matches!(self, DownloadStatus::Downloading | DownloadStatus::Paused | DownloadStatus::Pending)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunkInfo {
    pub chunk_hashes: Vec<String>,
    pub status: DownloadStatus,
    pub downloaded_chunks: Vec<String>,
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub total_chunks: usize,
    pub downloaded_chunks: usize,
    pub percentage: f64,
}

impl DownloadManifest {
    /// Erstellt ein neues Manifest aus deckdrop_chunks.toml
    pub fn from_chunks_toml(
        game_id: String,
        game_name: String,
        game_path: String,
        chunks_toml_content: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Parse deckdrop_chunks.toml
        let chunks_data: Vec<ChunkFileEntry> = toml::from_str(chunks_toml_content)?;
        
        let mut chunks = HashMap::new();
        let mut total_chunks = 0;
        
        for entry in chunks_data {
            let chunk_count = entry.chunk_count as usize;
            total_chunks += chunk_count;
            
            // Generiere Chunk-Hashes dynamisch basierend auf Position
            // Format: "{file_hash}:{chunk_index}" für eindeutige Identifikation
            let chunk_hashes: Vec<String> = (0..chunk_count)
                .map(|i| format!("{}:{}", entry.file_hash, i))
                .collect();
            
            chunks.insert(
                entry.path.clone(),
                FileChunkInfo {
                    chunk_hashes,
                    status: DownloadStatus::Pending,
                    downloaded_chunks: Vec::new(),
                    file_size: Some(entry.file_size as u64),
                },
            );
        }
        
        Ok(DownloadManifest {
            game_id,
            game_name,
            game_path,
            chunks,
            overall_status: DownloadStatus::Pending,
            progress: DownloadProgress {
                total_chunks,
                downloaded_chunks: 0,
                percentage: 0.0,
            },
        })
    }
    
    /// Speichert das Manifest
    pub fn save(&self, manifest_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let json = serde_json::to_string_pretty(self)?;
        fs::write(manifest_path, json)?;
        
        Ok(())
    }
    
    /// Lädt ein Manifest
    pub fn load(manifest_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(manifest_path)?;
        let manifest: DownloadManifest = serde_json::from_str(&content)?;
        Ok(manifest)
    }
    
    /// Ermittelt fehlende Chunks
    pub fn get_missing_chunks(&self) -> Vec<String> {
        let mut missing = Vec::new();
        
        for file_info in self.chunks.values() {
            for chunk_hash in &file_info.chunk_hashes {
                if !file_info.downloaded_chunks.contains(chunk_hash) {
                    if !missing.contains(chunk_hash) {
                        missing.push(chunk_hash.clone());
                    }
                }
            }
        }
        
        missing
    }
    
    /// Aktualisiert den Status nach dem Download eines Chunks
    pub fn mark_chunk_downloaded(&mut self, chunk_hash: &str) {
        for file_info in self.chunks.values_mut() {
            if file_info.chunk_hashes.contains(&chunk_hash.to_string()) {
                if !file_info.downloaded_chunks.contains(&chunk_hash.to_string()) {
                    file_info.downloaded_chunks.push(chunk_hash.to_string());
                }
            }
        }
        
        // Aktualisiere Gesamt-Progress
        let mut total_downloaded = 0;
        for file_info in self.chunks.values() {
            total_downloaded += file_info.downloaded_chunks.len();
        }
        
        self.progress.downloaded_chunks = total_downloaded;
        if self.progress.total_chunks > 0 {
            self.progress.percentage = (total_downloaded as f64 / self.progress.total_chunks as f64) * 100.0;
        }
        
        // Prüfe ob alle Chunks einer Datei vorhanden sind
        for file_info in self.chunks.values_mut() {
            if file_info.downloaded_chunks.len() == file_info.chunk_hashes.len() {
                file_info.status = DownloadStatus::Complete;
            } else {
                file_info.status = DownloadStatus::Downloading;
            }
        }
        
        // Prüfe ob alle Dateien komplett sind
        let all_complete = self.chunks.values()
            .all(|fi| fi.status == DownloadStatus::Complete);
        
        if all_complete {
            self.overall_status = DownloadStatus::Complete;
        } else {
            self.overall_status = DownloadStatus::Downloading;
        }
    }
}

/// Struktur für einen Eintrag in deckdrop_chunks.toml
/// Verwendet i64 statt usize/u64 für TOML-Kompatibilität
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkFileEntry {
    path: String,
    file_hash: String,      // SHA-256 Hash der gesamten Datei
    chunk_count: i64,       // Anzahl der 100MB Chunks (i64 für TOML)
    file_size: i64,         // Dateigröße in Bytes (i64 für TOML)
}

/// Speichert einen Chunk temporär
pub fn save_chunk(chunk_hash: &str, chunk_data: &[u8], chunks_dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    fs::create_dir_all(chunks_dir)?;
    
    // Neues Format: "{file_hash}:{chunk_index}" - verwende gesamten String als Dateiname
    // Ersetze ":" durch "_" für Dateinamen-Kompatibilität
    let safe_hash_name = chunk_hash.replace(':', "_");
    let chunk_path = chunks_dir.join(format!("{}.chunk", safe_hash_name));
    
    fs::write(&chunk_path, chunk_data)?;
    
    // Keine Hash-Validierung hier - wird später bei der Datei-Rekonstruktion validiert
    // (da wir nur den file_hash haben, nicht den Chunk-Hash)
    
    Ok(chunk_path)
}

/// Lädt einen gespeicherten Chunk
pub fn load_chunk(chunk_hash: &str, chunks_dir: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Neues Format: "{file_hash}:{chunk_index}" - ersetze ":" durch "_" für Dateinamen
    let safe_hash_name = chunk_hash.replace(':', "_");
    let chunk_path = chunks_dir.join(format!("{}.chunk", safe_hash_name));
    
    let data = fs::read(&chunk_path)?;
    Ok(data)
}

/// Rekonstruiert eine Datei aus ihren Chunks
pub fn reconstruct_file(
    file_path: &str,
    chunk_hashes: &[String],
    chunks_dir: &Path,
    output_path: &Path,
    expected_file_hash: &str,
    expected_file_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Stelle sicher, dass das Ausgabeverzeichnis existiert
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let mut file = fs::File::create(output_path)?;
    use std::io::Write;
    
    // Lade Chunks in der richtigen Reihenfolge (sortiert nach Index)
    let mut chunk_indices: Vec<(usize, &String)> = chunk_hashes
        .iter()
        .enumerate()
        .map(|(i, hash)| {
            // Extrahiere Index aus Hash-Format "{file_hash}:{index}"
            let index = hash.split(':').last().and_then(|s| s.parse::<usize>().ok()).unwrap_or(i);
            (index, hash)
        })
        .collect();
    chunk_indices.sort_by_key(|(idx, _)| *idx);
    
    for (_index, chunk_hash) in chunk_indices {
        let chunk_data = load_chunk(chunk_hash, chunks_dir)?;
        file.write_all(&chunk_data)?;
    }
    
    // Validiere die rekonstruierte Datei mit dem file_hash
    let mut hasher = Sha256::new();
    let file_data = fs::read(output_path)?;
    
    // Prüfe Dateigröße
    if file_data.len() as u64 != expected_file_size {
        return Err(format!("Dateigröße stimmt nicht überein: erwartet {}, erhalten {}", expected_file_size, file_data.len()).into());
    }
    
    // Prüfe Hash
    hasher.update(&file_data);
    let computed_hash = hasher.finalize();
    let computed_hex = hex::encode(computed_hash);
    
    if computed_hex != expected_file_hash {
        return Err(format!("Hash-Validierung fehlgeschlagen für Datei {}: erwartet {}, erhalten {}", file_path, expected_file_hash, computed_hex).into());
    }
    
    Ok(())
}

/// Ermittelt den Manifest-Pfad für ein Spiel
pub fn get_manifest_path(game_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
        .ok_or("Konnte Konfigurationsverzeichnis nicht bestimmen")?;
    let config_dir = base_dir.config_dir();
    let games_dir = config_dir.join("games").join(game_id);
    Ok(games_dir.join("manifest.json"))
}

/// Ermittelt den Chunks-Verzeichnis-Pfad für ein Spiel
pub fn get_chunks_dir(game_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
        .ok_or("Konnte Konfigurationsverzeichnis nicht bestimmen")?;
    let config_dir = base_dir.config_dir();
    let games_dir = config_dir.join("games").join(game_id);
    Ok(games_dir.join("chunks"))
}

/// Startet den Download-Prozess für ein Spiel
pub fn start_game_download(
    game_id: &str,
    deckdrop_toml: &str,
    deckdrop_chunks_toml: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse deckdrop.toml um game_name zu erhalten
    let game_info: crate::game::GameInfo = toml::from_str(deckdrop_toml)?;
    let game_name = game_info.name.clone();
    
    // Bestimme Ziel-Pfad (aus Config)
    let config = crate::config::Config::load();
    let game_path = config.download_path.join(&game_name);
    
    // Erstelle Manifest
    let manifest = DownloadManifest::from_chunks_toml(
        game_id.to_string(),
        game_name,
        game_path.to_string_lossy().to_string(),
        deckdrop_chunks_toml,
    )?;
    
    // Speichere Manifest
    let manifest_path = get_manifest_path(game_id)?;
    manifest.save(&manifest_path)?;
    
    // Speichere auch deckdrop.toml und deckdrop_chunks.toml im Manifest-Verzeichnis
    if let Some(manifest_dir) = manifest_path.parent() {
        std::fs::create_dir_all(manifest_dir)?;
        std::fs::write(manifest_dir.join("deckdrop.toml"), deckdrop_toml)?;
        std::fs::write(manifest_dir.join("deckdrop_chunks.toml"), deckdrop_chunks_toml)?;
    }
    
    println!("Download gestartet für Spiel: {} (ID: {})", game_info.name, game_id);
    eprintln!("Download gestartet für Spiel: {} (ID: {})", game_info.name, game_id);
    
    Ok(())
}

/// Fordert fehlende Chunks für ein Spiel an
pub fn request_missing_chunks(
    game_id: &str,
    peer_ids: &[String],
    download_request_tx: &tokio::sync::mpsc::UnboundedSender<deckdrop_network::network::discovery::DownloadRequest>,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = get_manifest_path(game_id)?;
    let manifest = DownloadManifest::load(&manifest_path)?;
    
    let missing_chunks = manifest.get_missing_chunks();
    
    if missing_chunks.is_empty() {
        println!("Keine fehlenden Chunks für Spiel {}", game_id);
        return Ok(());
    }
    
    println!("Fordere {} fehlende Chunks für Spiel {} an", missing_chunks.len(), game_id);
    eprintln!("Fordere {} fehlende Chunks für Spiel {} an", missing_chunks.len(), game_id);
    
    // Verteile Chunks auf verfügbare Peers (Round-Robin)
    for (i, chunk_hash) in missing_chunks.iter().enumerate() {
        let peer_id = &peer_ids[i % peer_ids.len()];
        
        if let Err(e) = download_request_tx.send(
            deckdrop_network::network::discovery::DownloadRequest::RequestChunk {
                peer_id: peer_id.clone(),
                chunk_hash: chunk_hash.clone(),
                game_id: game_id.to_string(),
            }
        ) {
            eprintln!("Fehler beim Senden von Chunk-Request für {}: {}", chunk_hash, e);
        } else {
            println!("Chunk-Request gesendet: {} von Peer {}", chunk_hash, peer_id);
        }
    }
    
    Ok(())
}

/// Findet die game_id für einen Chunk durch Suche in allen Manifesten
pub fn find_game_id_for_chunk(chunk_hash: &str) -> Result<String, Box<dyn std::error::Error>> {
    let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
        .ok_or("Konnte Konfigurationsverzeichnis nicht bestimmen")?;
    let config_dir = base_dir.config_dir();
    let games_dir = config_dir.join("games");
    
    if !games_dir.exists() {
        return Err("Games-Verzeichnis existiert nicht".into());
    }
    
    // Durchsuche alle Spiel-Verzeichnisse
    for entry in std::fs::read_dir(&games_dir)? {
        let entry = entry?;
        let manifest_path = entry.path().join("manifest.json");
        
        if manifest_path.exists() {
            if let Ok(manifest) = DownloadManifest::load(&manifest_path) {
                // Prüfe ob dieser Chunk im Manifest ist
                for file_info in manifest.chunks.values() {
                    if file_info.chunk_hashes.contains(&chunk_hash.to_string()) {
                        return Ok(manifest.game_id);
                    }
                }
            }
        }
    }
    
    Err(format!("Kein Manifest mit Chunk {} gefunden", chunk_hash).into())
}

/// Prüft ob Dateien komplett sind und rekonstruiert sie
pub fn check_and_reconstruct_files(
    game_id: &str,
    manifest: &DownloadManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let chunks_dir = get_chunks_dir(game_id)?;
    
    // Lade deckdrop_chunks.toml um file_hash zu erhalten
    let manifest_path = get_manifest_path(game_id)?;
    let chunks_toml_path = manifest_path.parent()
        .ok_or("Konnte Manifest-Verzeichnis nicht finden")?
        .join("deckdrop_chunks.toml");
    
    let chunks_toml_content = fs::read_to_string(&chunks_toml_path)?;
    let chunks_data: Vec<ChunkFileEntry> = toml::from_str(&chunks_toml_content)?;
    
    // Erstelle HashMap für schnellen Zugriff auf file_hash
    let file_hashes: HashMap<String, (String, u64)> = chunks_data
        .into_iter()
        .map(|e| (e.path.clone(), (e.file_hash, e.file_size as u64)))
        .collect();
    
    for (file_path, file_info) in &manifest.chunks {
        if file_info.status == DownloadStatus::Complete 
            && file_info.downloaded_chunks.len() == file_info.chunk_hashes.len() {
            // Datei ist komplett - rekonstruiere sie
            let output_path = PathBuf::from(&manifest.game_path).join(file_path);
            
            if !output_path.exists() {
                // Hole file_hash und file_size
                if let Some((file_hash, file_size)) = file_hashes.get(file_path) {
                    // Rekonstruiere Datei nur wenn sie noch nicht existiert
                    if let Err(e) = reconstruct_file(
                        file_path,
                        &file_info.chunk_hashes,
                        &chunks_dir,
                        &output_path,
                        file_hash,
                        *file_size,
                    ) {
                        eprintln!("Fehler beim Rekonstruieren von {}: {}", file_path, e);
                    } else {
                        println!("Datei rekonstruiert: {}", file_path);
                        eprintln!("Datei rekonstruiert: {}", file_path);
                    }
                } else {
                    eprintln!("Konnte file_hash für {} nicht finden", file_path);
                }
            }
        }
    }
    
    Ok(())
}

/// Finalisiert den Download eines Spiels
pub fn finalize_game_download(
    game_id: &str,
    manifest: &DownloadManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let game_path = PathBuf::from(&manifest.game_path);
    let manifest_path = get_manifest_path(game_id)?;
    
    // Stelle sicher, dass das Spielverzeichnis existiert
    std::fs::create_dir_all(&game_path)?;
    
    // Kopiere Metadaten
    if let Some(manifest_dir) = manifest_path.parent() {
        let deckdrop_toml_src = manifest_dir.join("deckdrop.toml");
        let deckdrop_chunks_toml_src = manifest_dir.join("deckdrop_chunks.toml");
        
        if deckdrop_toml_src.exists() {
            std::fs::copy(&deckdrop_toml_src, game_path.join("deckdrop.toml"))?;
        }
        if deckdrop_chunks_toml_src.exists() {
            std::fs::copy(&deckdrop_chunks_toml_src, game_path.join("deckdrop_chunks.toml"))?;
        }
    }
    
    // Füge zur Bibliothek hinzu
    let mut config = crate::config::Config::load();
    config.add_game_path(&game_path)?;
    
    println!("Spiel-Download abgeschlossen: {} (ID: {})", manifest.game_name, game_id);
    eprintln!("Spiel-Download abgeschlossen: {} (ID: {})", manifest.game_name, game_id);
    
    Ok(())
}

/// Bricht einen Download ab und löscht alle Daten
pub fn cancel_game_download(game_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = get_manifest_path(game_id)?;
    let manifest_dir = manifest_path.parent()
        .ok_or("Konnte Manifest-Verzeichnis nicht bestimmen")?;
    
    // Markiere als abgebrochen
    if let Ok(mut manifest) = DownloadManifest::load(&manifest_path) {
        manifest.overall_status = DownloadStatus::Cancelled;
        let _ = manifest.save(&manifest_path);
    }
    
    // Lösche alle Download-Daten
    if manifest_dir.exists() {
        std::fs::remove_dir_all(manifest_dir)?;
    }
    
    println!("Download abgebrochen und Daten gelöscht für Spiel: {}", game_id);
    eprintln!("Download abgebrochen und Daten gelöscht für Spiel: {}", game_id);
    
    Ok(())
}
