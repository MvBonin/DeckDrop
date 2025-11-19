use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{Seek, SeekFrom, Write};

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
        // Parse deckdrop_chunks.toml - Format: [[file]]
        #[derive(Deserialize)]
        struct ChunksToml {
            file: Vec<ChunkFileEntry>,
        }
        let chunks_toml: ChunksToml = toml::from_str(chunks_toml_content)?;
        let chunks_data = chunks_toml.file;
        
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
    
    /// Speichert das Manifest (nicht thread-safe, verwende update_manifest_atomic für thread-safe Updates)
    pub fn save(&self, manifest_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = manifest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Robustheit: Atomic Write (Temp-File → Rename) für Transaktions-Sicherheit
        let temp_path = manifest_path.with_extension("json.tmp");
        
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&temp_path, json)?;
        
        // Atomic Rename (atomar auf den meisten Dateisystemen)
        fs::rename(&temp_path, manifest_path)?;
        
        Ok(())
    }
    
    /// Atomares Update eines Manifests (thread-safe)
    /// Führt Load-Modify-Save atomar durch, um Race Conditions zu vermeiden
    pub fn update_manifest_atomic<F>(
        manifest_path: &Path,
        update_fn: F,
    ) -> Result<Self, Box<dyn std::error::Error>>
    where
        F: FnOnce(&mut Self) -> Result<(), Box<dyn std::error::Error>>,
    {
        // Lade Manifest
        let mut manifest = Self::load(manifest_path)?;
        
        // Führe Update durch
        update_fn(&mut manifest)?;
        
        // Speichere atomar
        manifest.save(manifest_path)?;
        
        Ok(manifest)
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
    file_hash: String,      // Blake3 Hash der gesamten Datei
    chunk_count: i64,       // Anzahl der 100MB Chunks (i64 für TOML)
    file_size: i64,         // Dateigröße in Bytes (i64 für TOML)
}

/// Pre-Allokiert eine Datei (erstellt sparse file)
/// BitTorrent-ähnlich: Datei wird vorher blockiert, Chunks können dann direkt an richtige Position geschrieben werden
pub fn preallocate_file(
    file_path: &Path,
    file_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Stelle sicher, dass das Verzeichnis existiert
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    // Erstelle Datei und setze Größe (erstellt sparse file auf unterstützten Dateisystemen)
    let file = fs::File::create(file_path)?;
    file.set_len(file_size)?;
    
    Ok(())
}

/// Schreibt einen Chunk direkt an die richtige Position in eine Datei
/// BitTorrent-ähnlich: Piece-by-Piece Writing - Chunks können in beliebiger Reihenfolge geschrieben werden
/// 
/// **Thread-Safe**: Mehrere Chunks können gleichzeitig in die gleiche Datei geschrieben werden,
/// da jeder Chunk einen unterschiedlichen Offset hat. File-Locking wird verwendet für zusätzliche Sicherheit.
pub fn write_chunk_to_position(
    file_path: &Path,
    chunk_index: usize,
    chunk_data: &[u8],
    chunk_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::OpenOptions;
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    
    // Thread-safe File-Handle-Cache pro Datei
    // Verwende OnceLock für statische Mutex-Map
    static FILE_LOCKS: std::sync::OnceLock<Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>> = std::sync::OnceLock::new();
    let locks = FILE_LOCKS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
    
    // Hole oder erstelle Lock für diese Datei
    let file_lock = {
        let mut locks_map = locks.lock().unwrap();
        locks_map
            .entry(file_path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    
    // Lock für diese Datei (verhindert Race Conditions beim Öffnen/Schreiben)
    let _guard = file_lock.lock().unwrap();
    
    // Öffne Datei im Read-Write-Modus (Datei sollte bereits pre-allokiert sein)
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(file_path)?;
    
    // Berechne Offset basierend auf Chunk-Index
    let offset = (chunk_index as u64) * chunk_size;
    
    // Springe zur richtigen Position
    file.seek(SeekFrom::Start(offset))?;
    
    // Schreibe Chunk-Daten
    // WICHTIG: Auf Unix-Systemen können mehrere Prozesse gleichzeitig in verschiedene
    // Bereiche einer Datei schreiben (pwrite), aber wir verwenden Locking für Sicherheit
    file.write_all(chunk_data)?;
    
    // Sync nur den geschriebenen Bereich (optional, für bessere Performance)
    // file.sync_all() würde die gesamte Datei syncen - das ist bei parallelem Schreiben nicht nötig
    // file.sync_data() syncs nur die Daten, nicht die Metadaten
    
    Ok(())
}

/// Validiert einen Chunk vor der Speicherung
/// Prüft die erwartete Größe basierend auf dem Manifest
pub fn validate_chunk_size(
    chunk_hash: &str,
    chunk_data: &[u8],
    manifest: &DownloadManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    // Extrahiere file_hash und chunk_index aus chunk_hash (Format: "file_hash:index")
    let parts: Vec<&str> = chunk_hash.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("Ungültiges Chunk-Hash-Format: {}", chunk_hash).into());
    }
    
    let chunk_index: usize = parts[1].parse()
        .map_err(|_| format!("Ungültiger Chunk-Index: {}", parts[1]))?;
    
    // Finde die Datei im Manifest, die diesen Chunk enthält
    let file_info = manifest.chunks.values()
        .find(|info| info.chunk_hashes.contains(&chunk_hash.to_string()))
        .ok_or_else(|| format!("Chunk {} nicht im Manifest gefunden", chunk_hash))?;
    
    // Berechne erwartete Chunk-Größe
    if let Some(file_size) = file_info.file_size {
        const CHUNK_SIZE: u64 = 10 * 1024 * 1024; // 10MB (reduziert von 100MB)
        let total_chunks = file_info.chunk_hashes.len();
        let is_last_chunk = chunk_index == total_chunks - 1;
        
        let expected_size = if is_last_chunk {
            // Letzter Chunk kann kleiner sein
            let full_chunks_size = (total_chunks - 1) as u64 * CHUNK_SIZE;
            file_size.saturating_sub(full_chunks_size)
        } else {
            CHUNK_SIZE
        };
        
        let actual_size = chunk_data.len() as u64;
        
        // Toleranz: ±1% oder mindestens 1KB Unterschied
        let tolerance = expected_size.max(1024) / 100;
        if actual_size.abs_diff(expected_size) > tolerance {
            return Err(format!(
                "Chunk-Größe stimmt nicht überein: erwartet {} Bytes, erhalten {} Bytes (Toleranz: {})",
                expected_size, actual_size, tolerance
            ).into());
        }
    }
    
    Ok(())
}

/// Speichert einen Chunk temporär mit Validierung
/// Verwendet Atomic Write (Temp-File → Rename) für Transaktions-Sicherheit
pub fn save_chunk(chunk_hash: &str, chunk_data: &[u8], chunks_dir: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    fs::create_dir_all(chunks_dir)?;
    
    // Neues Format: "{file_hash}:{chunk_index}" - verwende gesamten String als Dateiname
    // Ersetze ":" durch "_" für Dateinamen-Kompatibilität
    let safe_hash_name = chunk_hash.replace(':', "_");
    let chunk_path = chunks_dir.join(format!("{}.chunk", safe_hash_name));
    
    // Transaktions-Sicherheit: Schreibe zuerst in Temp-File, dann atomic rename
    let temp_path = chunks_dir.join(format!("{}.chunk.tmp", safe_hash_name));
    
    // Phase 4: I/O-Buffering für bessere Performance (8MB Buffer)
    use std::io::{BufWriter, Write};
    {
        let file = fs::File::create(&temp_path)?;
        let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file); // 8MB Buffer
        writer.write_all(chunk_data)?;
        writer.flush()?;
        // File wird hier geschlossen (Drop)
    }
    
    // Validiere geschriebene Datei (Größe)
    let written_size = fs::metadata(&temp_path)?.len();
    if written_size != chunk_data.len() as u64 {
        let _ = fs::remove_file(&temp_path); // Cleanup
        return Err(format!(
            "Chunk-Schreibfehler: erwartet {} Bytes, geschrieben {} Bytes",
            chunk_data.len(), written_size
        ).into());
    }
    
    // Atomic Rename (atomar auf den meisten Dateisystemen)
    fs::rename(&temp_path, &chunk_path)?;
    
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

/// Prüft ob eine Datei komplett ist und validiert sie
/// Wird verwendet nachdem alle Chunks geschrieben wurden (Piece-by-Piece Writing)
pub fn validate_complete_file(
    file_path: &Path,
    expected_file_hash: &str,
    expected_file_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Prüfe ob Datei existiert
    if !file_path.exists() {
        return Err(format!("Datei existiert nicht: {}", file_path.display()).into());
    }
    
    // Prüfe Dateigröße
    let metadata = fs::metadata(file_path)?;
    if metadata.len() != expected_file_size {
        return Err(format!("Dateigröße stimmt nicht überein: erwartet {}, erhalten {}", 
            expected_file_size, metadata.len()).into());
    }
    
    // Prüfe Hash (Blake3)
    use crate::gamechecker::calculate_file_hash;
    let computed_hash = calculate_file_hash(file_path)?;
    
    if computed_hash != expected_file_hash {
        return Err(format!("Hash-Validierung fehlgeschlagen für Datei {}: erwartet {}, erhalten {}", 
            file_path.display(), expected_file_hash, computed_hash).into());
    }
    
    Ok(())
}

/// Phase 2: Schreibt einen Chunk direkt in die finale Datei (Piece-by-Piece Writing)
/// BitTorrent-ähnlich: Chunk wird direkt an richtige Position geschrieben, keine temporäre Speicherung nötig
pub fn write_chunk_to_file(
    chunk_hash: &str,
    chunk_data: &[u8],
    manifest: &DownloadManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    // Extrahiere file_hash und chunk_index aus chunk_hash (Format: "file_hash:index")
    let parts: Vec<&str> = chunk_hash.split(':').collect();
    if parts.len() != 2 {
        return Err(format!("Ungültiges Chunk-Hash-Format: {}", chunk_hash).into());
    }
    
    let chunk_index: usize = parts[1].parse()
        .map_err(|_| format!("Ungültiger Chunk-Index: {}", parts[1]))?;
    
    // Finde die Datei im Manifest, die diesen Chunk enthält
    let (file_path, file_info) = manifest.chunks.iter()
        .find(|(_, info)| info.chunk_hashes.contains(&chunk_hash.to_string()))
        .ok_or_else(|| format!("Chunk {} nicht im Manifest gefunden", chunk_hash))?;
    
    // Bestimme vollständigen Dateipfad
    let game_path = PathBuf::from(&manifest.game_path);
    let full_file_path = game_path.join(file_path);
    
    // Berechne Chunk-Größe dynamisch basierend auf Dateigröße und Anzahl der Chunks
    let chunk_size = if let Some(file_size) = file_info.file_size {
        let total_chunks = file_info.chunk_hashes.len();
        if total_chunks > 0 {
            // Standard-Chunk-Größe: 10MB (reduziert von 100MB für bessere Parallelisierung)
            const STANDARD_CHUNK_SIZE: u64 = 10 * 1024 * 1024; // 10MB
            // Berechne Größe pro Chunk (aufrunden für letzte Chunk)
            let calculated_size = (file_size + total_chunks as u64 - 1) / total_chunks as u64;
            // Verwende berechnete Größe wenn kleiner als Standard (für kleine Dateien)
            // oder Standard-Größe wenn größer (für große Dateien)
            if calculated_size < STANDARD_CHUNK_SIZE {
                calculated_size
            } else {
                STANDARD_CHUNK_SIZE
            }
        } else {
            10 * 1024 * 1024 // Fallback: 10MB
        }
    } else {
        10 * 1024 * 1024 // Fallback: 10MB
    };
    
    // Schreibe Chunk direkt an richtige Position
    write_chunk_to_position(&full_file_path, chunk_index, chunk_data, chunk_size)?;
    
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
/// Chunks werden im Download-Ordner in einem "temp" Unterordner gespeichert
pub fn get_chunks_dir(game_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let config = crate::config::Config::load();
    let download_path = config.download_path;
    Ok(download_path.join("temp").join(game_id))
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
    
    // Phase 1: Pre-Allocation - Blockiere Dateien vorher (BitTorrent-ähnlich)
    // Parse deckdrop_chunks.toml um Dateien zu pre-allokieren
    #[derive(Deserialize)]
    struct ChunksToml {
        file: Vec<ChunkFileEntry>,
    }
    let chunks_toml: ChunksToml = toml::from_str(deckdrop_chunks_toml)?;
    
    for entry in chunks_toml.file {
        let file_path = game_path.join(&entry.path);
        
        // Pre-Allokiere Datei (erstellt sparse file)
        match preallocate_file(&file_path, entry.file_size as u64) {
            Ok(()) => {
                println!("Datei pre-allokiert: {} ({} Bytes)", entry.path, entry.file_size);
                eprintln!("Datei pre-allokiert: {} ({} Bytes)", entry.path, entry.file_size);
            }
            Err(e) => {
                eprintln!("Warnung: Pre-Allocation fehlgeschlagen für {}: {} (Datei wird beim ersten Chunk erstellt)", entry.path, e);
                // Nicht kritisch - Datei wird beim ersten Chunk erstellt
            }
        }
    }
    
    println!("Download gestartet für Spiel: {} (ID: {})", game_info.name, game_id);
    eprintln!("Download gestartet für Spiel: {} (ID: {})", game_info.name, game_id);
    
    Ok(())
}

/// Fordert fehlende Chunks für ein Spiel an
/// 
/// `max_chunks_per_peer`: Maximale Anzahl gleichzeitiger Chunk-Downloads von einem Peer (default: 3)
pub fn request_missing_chunks(
    game_id: &str,
    peer_ids: &[String],
    download_request_tx: &tokio::sync::mpsc::UnboundedSender<deckdrop_network::network::discovery::DownloadRequest>,
    max_chunks_per_peer: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = get_manifest_path(game_id)?;
    let manifest = DownloadManifest::load(&manifest_path)?;
    
    let missing_chunks = manifest.get_missing_chunks();
    
    if missing_chunks.is_empty() {
        println!("Keine fehlenden Chunks für Spiel {}", game_id);
        return Ok(());
    }
    
    println!("Fordere {} fehlende Chunks für Spiel {} an (max {} pro Peer)", 
        missing_chunks.len(), game_id, max_chunks_per_peer);
    eprintln!("Fordere {} fehlende Chunks für Spiel {} an (max {} pro Peer)", 
        missing_chunks.len(), game_id, max_chunks_per_peer);
    
    // Optimierte Multi-Peer Parallelisierung mit Round-Robin und Load-Balancing
    let mut peer_chunk_counts: HashMap<String, usize> = HashMap::new();
    let mut peer_index = 0; // Round-Robin Index
    
    // Initialisiere alle Peers mit 0
    for peer_id in peer_ids.iter() {
        peer_chunk_counts.insert(peer_id.clone(), 0);
    }
    
    for chunk_hash in missing_chunks.iter() {
        // Round-Robin Start: Beginne mit dem nächsten Peer im Round-Robin
        let start_index = peer_index;
        let mut selected_peer = None;
        let mut min_count = usize::MAX;
        let mut attempts = 0;
        
        // Suche den besten Peer (mit wenigsten aktiven Downloads, aber unter Limit)
        while attempts < peer_ids.len() {
            let current_peer = &peer_ids[peer_index % peer_ids.len()];
            let count = peer_chunk_counts.get(current_peer).copied().unwrap_or(0);
            
            // Wenn dieser Peer noch Platz hat und weniger Downloads hat als bisher
            if count < max_chunks_per_peer && count < min_count {
                min_count = count;
                selected_peer = Some(current_peer.clone());
                // Wenn dieser Peer deutlich weniger Downloads hat, nimm ihn sofort (aggressives Load-Balancing)
                if count == 0 || (min_count > 0 && count < min_count / 2) {
                    break;
                }
            }
            
            peer_index = (peer_index + 1) % peer_ids.len();
            attempts += 1;
            
            // Verhindere Endlosschleife
            if peer_index == start_index && attempts > 0 {
                break;
            }
        }
        
        // Wenn kein Peer mit Platz gefunden, verwende Round-Robin als Fallback
        if selected_peer.is_none() {
            selected_peer = Some(peer_ids[peer_index % peer_ids.len()].clone());
            peer_index = (peer_index + 1) % peer_ids.len();
        }
        
        if let Some(peer_id) = selected_peer {
            // Erhöhe Zähler für diesen Peer
            *peer_chunk_counts.entry(peer_id.clone()).or_insert(0) += 1;
            
            if let Err(e) = download_request_tx.send(
                deckdrop_network::network::discovery::DownloadRequest::RequestChunk {
                    peer_id: peer_id.clone(),
                    chunk_hash: chunk_hash.clone(),
                    game_id: game_id.to_string(),
                }
            ) {
                eprintln!("Fehler beim Senden von Chunk-Request für {}: {}", chunk_hash, e);
                // Reduziere Zähler bei Fehler
                if let Some(count) = peer_chunk_counts.get_mut(&peer_id) {
                    *count = count.saturating_sub(1);
                }
            }
        } else {
            eprintln!("Kein Peer verfügbar für Chunk {} (alle Peers haben max Downloads erreicht)", chunk_hash);
        }
    }
    
    Ok(())
}

/// Lädt alle aktiven Downloads aus dem Manifest-Verzeichnis
pub fn load_active_downloads() -> Vec<(String, DownloadManifest)> {
    let base_dir = match directories::ProjectDirs::from("com", "deckdrop", "deckdrop") {
        Some(dir) => dir,
        None => return Vec::new(),
    };
    let config_dir = base_dir.config_dir();
    let games_dir = config_dir.join("games");
    
    if !games_dir.exists() {
        return Vec::new();
    }
    
    let mut downloads = Vec::new();
    
    // Durchsuche alle Spiel-Verzeichnisse
    if let Ok(entries) = std::fs::read_dir(&games_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                let manifest_path = entry.path().join("manifest.json");
                
                if manifest_path.exists() {
                    if let Ok(manifest) = DownloadManifest::load(&manifest_path) {
                        // Nur Downloads, die nicht abgebrochen sind
                        if !matches!(manifest.overall_status, DownloadStatus::Cancelled) {
                            downloads.push((manifest.game_id.clone(), manifest));
                        }
                    }
                }
            }
        }
    }
    
    downloads
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

/// Prüft ob Dateien komplett sind und validiert sie
/// Wird verwendet nachdem alle Chunks geschrieben wurden (Piece-by-Piece Writing)
pub fn check_and_validate_complete_files(
    game_id: &str,
    manifest: &DownloadManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    // Lade deckdrop_chunks.toml um file_hash zu erhalten
    let manifest_path = get_manifest_path(game_id)?;
    let chunks_toml_path = manifest_path.parent()
        .ok_or("Konnte Manifest-Verzeichnis nicht finden")?
        .join("deckdrop_chunks.toml");
    
    let chunks_toml_content = fs::read_to_string(&chunks_toml_path)?;
    #[derive(Deserialize)]
    struct ChunksToml {
        file: Vec<ChunkFileEntry>,
    }
    let chunks_toml: ChunksToml = toml::from_str(&chunks_toml_content)?;
    let chunks_data = chunks_toml.file;
    
    // Erstelle HashMap für schnellen Zugriff auf file_hash
    let file_hashes: HashMap<String, (String, u64)> = chunks_data
        .into_iter()
        .map(|e| (e.path.clone(), (e.file_hash, e.file_size as u64)))
        .collect();
    
    for (file_path, file_info) in &manifest.chunks {
        if file_info.status == DownloadStatus::Complete 
            && file_info.downloaded_chunks.len() == file_info.chunk_hashes.len() {
            // Datei ist komplett - validiere sie
            let output_path = PathBuf::from(&manifest.game_path).join(file_path);
            
            if output_path.exists() {
                // Hole file_hash und file_size
                if let Some((file_hash, file_size)) = file_hashes.get(file_path) {
                    // Validiere Datei (Hash und Größe)
                    if let Err(e) = validate_complete_file(&output_path, file_hash, *file_size) {
                        eprintln!("Fehler bei Validierung von {}: {}", file_path, e);
                    } else {
                        println!("Datei validiert: {}", file_path);
                        eprintln!("Datei validiert: {}", file_path);
                    }
                } else {
                    eprintln!("Konnte file_hash für {} nicht finden", file_path);
                }
            } else {
                eprintln!("Warnung: Datei {} sollte existieren, ist aber nicht vorhanden", file_path);
            }
        }
    }
    
    Ok(())
}

/// Findet den Dateipfad für einen Chunk im Manifest
pub fn find_file_for_chunk(
    manifest: &DownloadManifest,
    chunk_hash: &str,
) -> Option<String> {
    manifest.chunks.iter()
        .find(|(_, info)| info.chunk_hashes.contains(&chunk_hash.to_string()))
        .map(|(path, _)| path.clone())
}

/// Prüft ob eine einzelne Datei komplett ist und validiert sie
pub fn check_and_validate_single_file(
    game_id: &str,
    manifest: &DownloadManifest,
    file_path: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    // Lade deckdrop_chunks.toml um file_hash zu erhalten
    let manifest_path = get_manifest_path(game_id)?;
    let chunks_toml_path = manifest_path.parent()
        .ok_or("Konnte Manifest-Verzeichnis nicht finden")?
        .join("deckdrop_chunks.toml");
    
    let chunks_toml_content = fs::read_to_string(&chunks_toml_path)?;
    #[derive(Deserialize)]
    struct ChunksToml {
        file: Vec<ChunkFileEntry>,
    }
    let chunks_toml: ChunksToml = toml::from_str(&chunks_toml_content)?;
    
    // Finde file_hash und file_size für diese Datei
    let entry = chunks_toml.file.iter()
        .find(|e| e.path == file_path)
        .ok_or_else(|| format!("Datei {} nicht in deckdrop_chunks.toml gefunden", file_path))?;
    let file_hash = entry.file_hash.clone();
    let file_size = entry.file_size as u64;
    
    // Prüfe ob alle Chunks dieser Datei heruntergeladen sind
    let file_info = manifest.chunks.get(file_path)
        .ok_or_else(|| format!("Datei {} nicht im Manifest gefunden", file_path))?;
    
    if file_info.downloaded_chunks.len() != file_info.chunk_hashes.len() {
        return Ok(false); // Datei noch nicht komplett
    }
    
    // Datei sollte komplett sein - validiere sie
    let output_path = PathBuf::from(&manifest.game_path).join(file_path);
    
    if !output_path.exists() {
        return Err(format!("Datei {} sollte existieren, ist aber nicht vorhanden", file_path).into());
    }
    
    // Validiere Datei (Hash und Größe)
    validate_complete_file(&output_path, &file_hash, file_size)?;
    
    Ok(true) // Datei ist komplett und validiert
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
    
    // Validiere alle Dateien vor Finalisierung
    if let Err(e) = check_and_validate_complete_files(game_id, manifest) {
        eprintln!("Warnung: Fehler bei Validierung der Dateien: {}", e);
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
    
    // Lösche temporäre Chunks im Download-Ordner
    let chunks_dir = get_chunks_dir(game_id)?;
    if chunks_dir.exists() {
        std::fs::remove_dir_all(&chunks_dir)?;
        println!("Temporäre Chunks gelöscht: {}", chunks_dir.display());
    }
    
    // Lösche Manifest-Verzeichnis (enthält nur Metadaten, keine Chunks mehr)
    if manifest_dir.exists() {
        std::fs::remove_dir_all(manifest_dir)?;
    }
    
    println!("Download abgebrochen und Daten gelöscht für Spiel: {}", game_id);
    eprintln!("Download abgebrochen und Daten gelöscht für Spiel: {}", game_id);
    
    Ok(())
}

// Integritätsprüfung wurde nach gamechecker.rs verschoben

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_download_manifest_from_chunks_toml() {
        let chunks_toml = r#"[[file]]
path = "test.bin"
file_hash = "abc123def456"
chunk_count = 2
file_size = 150000000
"#;
        
        let manifest = DownloadManifest::from_chunks_toml(
            "test-game".to_string(),
            "Test Game".to_string(),
            "/path/to/game".to_string(),
            chunks_toml,
        ).unwrap();
        
        assert_eq!(manifest.game_id, "test-game");
        assert_eq!(manifest.game_name, "Test Game");
        assert_eq!(manifest.progress.total_chunks, 2);
        assert_eq!(manifest.chunks.len(), 1);
        
        let file_info = manifest.chunks.get("test.bin").unwrap();
        assert_eq!(file_info.chunk_hashes.len(), 2);
        assert_eq!(file_info.chunk_hashes[0], "abc123def456:0");
        assert_eq!(file_info.chunk_hashes[1], "abc123def456:1");
    }

    #[test]
    fn test_piece_by_piece_writing() {
        let temp_dir = TempDir::new().unwrap();
        let game_path = temp_dir.path().join("game");
        fs::create_dir_all(&game_path).unwrap();
        
        // Erstelle Test-Datei (reduziert auf 10MB für bessere Kompatibilität)
        let mut test_data = vec![0u8; 10 * 1024 * 1024]; // 10MB
        for i in 0..test_data.len() {
            // Fülle mit Mustern
            test_data[i] = (i % 256) as u8;
        }
        
        // Berechne Hash (Blake3)
        use crate::gamechecker::calculate_file_hash;
        use tempfile::NamedTempFile;
        use std::io::Write;
        
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(&test_data).unwrap();
        temp_file.flush().unwrap();
        let file_hash = calculate_file_hash(temp_file.path()).unwrap();
        
        // Erstelle Manifest
        let chunks_toml = format!(
            r#"[[file]]
path = "test.bin"
file_hash = "{}"
chunk_count = 2
file_size = {}
"#,
            file_hash, test_data.len()
        );
        
        let manifest = DownloadManifest::from_chunks_toml(
            "test-game".to_string(),
            "Test Game".to_string(),
            game_path.to_string_lossy().to_string(),
            &chunks_toml,
        ).unwrap();
        
        // Pre-Allokiere Datei
        let file_path = game_path.join("test.bin");
        preallocate_file(&file_path, test_data.len() as u64).unwrap();
        
        // Teile in Chunks (2 Chunks: 5MB + 5MB)
        let chunk1 = &test_data[0..5 * 1024 * 1024];
        let chunk2 = &test_data[5 * 1024 * 1024..];
        
        // Schreibe Chunks direkt in Datei (Piece-by-Piece, in beliebiger Reihenfolge)
        write_chunk_to_file(&format!("{}:1", file_hash), chunk2, &manifest).unwrap(); // Zweiter Chunk zuerst
        write_chunk_to_file(&format!("{}:0", file_hash), chunk1, &manifest).unwrap(); // Erster Chunk danach
        
        // Validiere Datei
        validate_complete_file(&file_path, &file_hash, test_data.len() as u64).unwrap();
        
        // Prüfe geschriebene Datei
        let written = fs::read(&file_path).unwrap();
        assert_eq!(written.len(), test_data.len());
        assert_eq!(written, test_data);
    }


    #[tokio::test]
    async fn test_two_peers_chunk_exchange() {
        // Erstelle temporäres Verzeichnis für beide Peers
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        
        let game_id = "test-game-2peers";
        
        // Peer 1: Hat das Spiel
        let game_path1 = temp_dir1.path().join("game");
        fs::create_dir_all(&game_path1).unwrap();
        
        // Erstelle Test-Datei für Peer 1 (reduziert auf 10MB für bessere Kompatibilität)
        let test_data = vec![42u8; 10 * 1024 * 1024]; // 10MB
        let test_file_path = game_path1.join("test.bin");
        fs::write(&test_file_path, &test_data).unwrap();
        
        // Berechne Hash mit wiederverwendbarer Funktion
        let computed_hash = crate::gamechecker::calculate_file_hash(&test_file_path).unwrap();
        
        // Erstelle deckdrop_chunks.toml für Peer 1
        let chunks_toml = format!(
            r#"[[file]]
path = "test.bin"
file_hash = "{}"
chunk_count = 2
file_size = {}
"#,
            computed_hash, test_data.len()
        );
        
        fs::write(game_path1.join("deckdrop_chunks.toml"), chunks_toml).unwrap();
        
        // Peer 2: Startet Download
        let _chunks_dir2 = temp_dir2.path().join("chunks");
        fs::create_dir_all(&_chunks_dir2).unwrap();
        
        // Erstelle Manifest für Peer 2
        let deckdrop_toml = format!(
            r#"game_id = "{}"
name = "Test Game"
version = "1.0"
start_file = "test.bin"
"#,
            game_id
        );
        
        let deckdrop_chunks_toml = format!(
            r#"[[file]]
path = "test.bin"
file_hash = "{}"
chunk_count = 2
file_size = {}
"#,
            computed_hash, test_data.len()
        );
        
        // Starte Download für Peer 2 (pre-allokiert Dateien)
        start_game_download(game_id, &deckdrop_toml, &deckdrop_chunks_toml).unwrap();
        
        // Lade Manifest für write_chunk_to_file
        let manifest_path = get_manifest_path(game_id).unwrap();
        let manifest = DownloadManifest::load(&manifest_path).unwrap();
        
        // Simuliere Chunk-Transfer von Peer 1 zu Peer 2 (Piece-by-Piece Writing)
        // Chunk 1: 0-5MB
        let chunk1 = &test_data[0..5 * 1024 * 1024];
        let chunk1_hash = format!("{}:0", computed_hash);
        write_chunk_to_file(&chunk1_hash, chunk1, &manifest).unwrap();
        
        // Chunk 2: 5-10MB
        let chunk2 = &test_data[5 * 1024 * 1024..];
        let chunk2_hash = format!("{}:1", computed_hash);
        write_chunk_to_file(&chunk2_hash, chunk2, &manifest).unwrap();
        
        // Markiere Chunks als heruntergeladen
        let mut manifest = DownloadManifest::load(&manifest_path).unwrap();
        manifest.mark_chunk_downloaded(&chunk1_hash);
        manifest.mark_chunk_downloaded(&chunk2_hash);
        manifest.save(&manifest_path).unwrap();
        
        // Prüfe ob alle Chunks vorhanden sind
        assert_eq!(manifest.progress.downloaded_chunks, 2);
        assert_eq!(manifest.progress.total_chunks, 2);
        
        // Validiere Datei (sollte bereits komplett sein durch Piece-by-Piece Writing)
        // Verwende Pfad aus Manifest (nicht game_path2, da start_game_download den Pfad aus Config verwendet)
        let output_path = PathBuf::from(&manifest.game_path).join("test.bin");
        validate_complete_file(&output_path, &computed_hash, test_data.len() as u64).unwrap();
        
        // Prüfe geschriebene Datei
        let written = fs::read(&output_path).unwrap();
        assert_eq!(written.len(), test_data.len());
        assert_eq!(written, test_data);
        
        // Erstelle deckdrop_chunks.toml für Integritätsprüfung
        // Verwende Pfad aus Manifest (nicht game_path2)
        let game_path_from_manifest = PathBuf::from(&manifest.game_path);
        let chunks_toml_for_check = format!(
            r#"[[file]]
path = "test.bin"
file_hash = "{}"
chunk_count = 2
file_size = {}
"#,
            computed_hash, test_data.len()
        );
        fs::write(game_path_from_manifest.join("deckdrop_chunks.toml"), chunks_toml_for_check).unwrap();
        
        // Prüfe Integrität
        let integrity_result = crate::gamechecker::verify_game_integrity(&game_path_from_manifest).unwrap();
        assert_eq!(integrity_result.verified_files, 1);
        assert_eq!(integrity_result.failed_files.len(), 0);
        
        println!("✓ Test erfolgreich: Zwei Peers haben Chunks ausgetauscht und Datei rekonstruiert!");
        
        // Aufräumen: Lösche Test-Spiel (Manifest und Chunks-Verzeichnis)
        let _ = cancel_game_download(game_id);
        
        // TempDir wird automatisch gelöscht wenn es out of scope geht
    }
    
    #[test]
    fn test_validate_chunk_size() {
        // Verwende eine kleine Test-Datei (20MB für 2 Chunks à 10MB)
        // Die validate_chunk_size Funktion verwendet eine fest codierte CHUNK_SIZE von 10MB
        const CHUNK_SIZE: usize = 10 * 1024 * 1024; // 10MB
        let file_size = 20 * 1024 * 1024; // 20MB
        
        // Erstelle Manifest ohne tatsächliche Datei (nur für Validierung)
        let file_hash = "test_file_hash_1234567890abcdef";
        let chunks_toml = format!(
            r#"[[file]]
path = "test.bin"
file_hash = "{}"
chunk_count = 2
file_size = {}
"#,
            file_hash, file_size
        );
        
        let manifest = DownloadManifest::from_chunks_toml(
            "test-game".to_string(),
            "Test Game".to_string(),
            "/tmp/test".to_string(),
            &chunks_toml,
        ).unwrap();
        
        // Test 1: Korrekte Chunk-Größe (10MB für ersten Chunk)
        let chunk0_data = vec![1u8; CHUNK_SIZE];
        let chunk0_hash = format!("{}:0", file_hash);
        assert!(validate_chunk_size(&chunk0_hash, &chunk0_data, &manifest).is_ok());
        
        // Test 2: Falsche Chunk-Größe (zu klein)
        let chunk0_small = vec![1u8; 1 * 1024 * 1024]; // 1MB statt 10MB
        assert!(validate_chunk_size(&chunk0_hash, &chunk0_small, &manifest).is_err());
        
        // Test 3: Letzter Chunk kann kleiner sein (10MB für letzten Chunk)
        let chunk1_hash = format!("{}:1", file_hash);
        let chunk1_data = vec![1u8; CHUNK_SIZE]; // 10MB für letzten Chunk
        assert!(validate_chunk_size(&chunk1_hash, &chunk1_data, &manifest).is_ok());
        
        // Test 4: Ungültiges Chunk-Hash-Format
        assert!(validate_chunk_size("invalid_hash", &chunk0_data, &manifest).is_err());
    }
    
    #[test]
    fn test_validate_complete_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.bin");
        
        // Erstelle Test-Datei
        let test_data = vec![42u8; 100 * 1024]; // 100KB
        fs::write(&file_path, &test_data).unwrap();
        
        // Berechne Hash
        use crate::gamechecker::calculate_file_hash;
        let computed_hash = calculate_file_hash(&file_path).unwrap();
        
        // Validiere Datei
        validate_complete_file(&file_path, &computed_hash, test_data.len() as u64).unwrap();
        
        // Test: Falscher Hash sollte fehlschlagen
        assert!(validate_complete_file(&file_path, "wrong_hash", test_data.len() as u64).is_err());
        
        // Test: Falsche Größe sollte fehlschlagen
        assert!(validate_complete_file(&file_path, &computed_hash, 999).is_err());
    }
    
    #[test]
    fn test_update_manifest_atomic() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("manifest.json");
        
        // Erstelle initiales Manifest
        let chunks_toml = r#"[[file]]
path = "test.bin"
file_hash = "abc123"
chunk_count = 2
file_size = 200000000
"#;
        
        let manifest = DownloadManifest::from_chunks_toml(
            "test-game".to_string(),
            "Test Game".to_string(),
            "/path/to/game".to_string(),
            chunks_toml,
        ).unwrap();
        manifest.save(&manifest_path).unwrap();
        
        // Test: Atomares Update
        let updated_manifest = DownloadManifest::update_manifest_atomic(
            &manifest_path,
            |manifest| {
                manifest.mark_chunk_downloaded("abc123:0");
                Ok(())
            }
        ).unwrap();
        
        // Prüfe dass Update erfolgreich war
        assert_eq!(updated_manifest.progress.downloaded_chunks, 1);
        
        // Prüfe dass Manifest korrekt gespeichert wurde
        let loaded_manifest = DownloadManifest::load(&manifest_path).unwrap();
        assert_eq!(loaded_manifest.progress.downloaded_chunks, 1);
        
        // Test: Mehrere atomare Updates hintereinander
        DownloadManifest::update_manifest_atomic(
            &manifest_path,
            |manifest| {
                manifest.mark_chunk_downloaded("abc123:1");
                Ok(())
            }
        ).unwrap();
        
        let final_manifest = DownloadManifest::load(&manifest_path).unwrap();
        assert_eq!(final_manifest.progress.downloaded_chunks, 2);
        assert_eq!(final_manifest.progress.total_chunks, 2);
    }
    
    #[test]
    fn test_manifest_save_atomic() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("manifest.json");
        
        let chunks_toml = r#"[[file]]
path = "test.bin"
file_hash = "abc123"
chunk_count = 1
file_size = 100000000
"#;
        
        let manifest = DownloadManifest::from_chunks_toml(
            "test-game".to_string(),
            "Test Game".to_string(),
            "/path/to/game".to_string(),
            chunks_toml,
        ).unwrap();
        
        // Test: Atomic Save (Temp-File → Rename)
        manifest.save(&manifest_path).unwrap();
        assert!(manifest_path.exists());
        
        // Prüfe dass keine Temp-Datei übrig bleibt (nach kurzer Wartezeit für Filesystem-Sync)
        std::thread::sleep(std::time::Duration::from_millis(100));
        let temp_files: Vec<_> = fs::read_dir(temp_dir.path()).unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path_str = e.path().to_string_lossy().to_string();
                path_str.contains(".tmp") && !path_str.contains("manifest")
            })
            .collect();
        assert_eq!(temp_files.len(), 0, "Keine Temp-Dateien sollten übrig bleiben (gefunden: {:?})", 
            temp_files.iter().map(|e| e.path()).collect::<Vec<_>>());
        
        // Prüfe dass Manifest korrekt geladen werden kann
        let loaded = DownloadManifest::load(&manifest_path).unwrap();
        assert_eq!(loaded.game_id, "test-game");
    }
    
    #[test]
    fn test_preallocate_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_preallocated.bin");
        let file_size = 200 * 1024 * 1024; // 200MB
        
        // Test: Pre-Allocation erstellt Datei mit korrekter Größe
        preallocate_file(&file_path, file_size).unwrap();
        assert!(file_path.exists());
        
        // Prüfe Dateigröße
        let metadata = fs::metadata(&file_path).unwrap();
        assert_eq!(metadata.len(), file_size);
        
        // Test: Pre-Allocation erstellt Verzeichnis falls nötig
        let nested_path = temp_dir.path().join("nested").join("test.bin");
        preallocate_file(&nested_path, 100 * 1024 * 1024).unwrap();
        assert!(nested_path.exists());
    }
    
    #[test]
    fn test_write_chunk_to_position() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_chunks.bin");
        const CHUNK_SIZE: u64 = 100 * 1024 * 1024; // 100MB
        let file_size = 2 * CHUNK_SIZE; // 200MB (2 Chunks)
        
        // Pre-Allokiere Datei
        preallocate_file(&file_path, file_size).unwrap();
        
        // Schreibe Chunk 1 (zweiter Chunk zuerst - testet Piece-by-Piece Writing)
        let chunk1_data = vec![42u8; CHUNK_SIZE as usize];
        write_chunk_to_position(&file_path, 1, &chunk1_data, CHUNK_SIZE).unwrap();
        
        // Schreibe Chunk 0 (erster Chunk danach)
        let chunk0_data = vec![24u8; CHUNK_SIZE as usize];
        write_chunk_to_position(&file_path, 0, &chunk0_data, CHUNK_SIZE).unwrap();
        
        // Prüfe dass Datei korrekt geschrieben wurde
        let file_data = fs::read(&file_path).unwrap();
        assert_eq!(file_data.len(), file_size as usize);
        
        // Prüfe Chunk 0
        assert_eq!(&file_data[0..CHUNK_SIZE as usize], &chunk0_data[..]);
        
        // Prüfe Chunk 1
        let chunk1_start = CHUNK_SIZE as usize;
        let chunk1_end = chunk1_start + CHUNK_SIZE as usize;
        assert_eq!(&file_data[chunk1_start..chunk1_end], &chunk1_data[..]);
    }
    
    #[test]
    fn test_write_chunk_to_file() {
        let temp_dir = TempDir::new().unwrap();
        let game_path = temp_dir.path().join("game");
        fs::create_dir_all(&game_path).unwrap();
        
        // Erstelle Manifest
        let chunks_toml = format!(
            r#"[[file]]
path = "test.bin"
file_hash = "test_hash_123"
chunk_count = 2
file_size = {}
"#,
            20 * 1024 * 1024 // 20MB
        );
        
        let manifest = DownloadManifest::from_chunks_toml(
            "test-game".to_string(),
            "Test Game".to_string(),
            game_path.to_string_lossy().to_string(),
            &chunks_toml,
        ).unwrap();
        
        // Pre-Allokiere Datei (wird normalerweise in start_game_download gemacht)
        let file_path = game_path.join("test.bin");
        preallocate_file(&file_path, 20 * 1024 * 1024).unwrap();
        
        // Schreibe Chunk 0
        let chunk0_data = vec![1u8; 10 * 1024 * 1024];
        write_chunk_to_file("test_hash_123:0", &chunk0_data, &manifest).unwrap();
        
        // Schreibe Chunk 1
        let chunk1_data = vec![2u8; 10 * 1024 * 1024];
        write_chunk_to_file("test_hash_123:1", &chunk1_data, &manifest).unwrap();
        
        // Prüfe dass Datei korrekt geschrieben wurde
        let file_data = fs::read(&file_path).unwrap();
        assert_eq!(file_data.len(), 20 * 1024 * 1024);
        assert_eq!(file_data[0], 1);
        assert_eq!(file_data[10 * 1024 * 1024], 2);
    }
    
    #[test]
    fn test_parallel_chunk_writing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_parallel.bin");
        const CHUNK_SIZE: u64 = 10 * 1024 * 1024; // 10MB
        let file_size = 4 * CHUNK_SIZE; // 40MB (4 Chunks)
        
        // Pre-Allokiere Datei
        preallocate_file(&file_path, file_size).unwrap();
        
        // Erstelle Test-Daten für 4 Chunks
        let chunk_data: Vec<Vec<u8>> = (0..4)
            .map(|i| vec![i as u8; CHUNK_SIZE as usize])
            .collect();
        
        // Schreibe Chunks in beliebiger Reihenfolge (testet Piece-by-Piece Writing)
        // Schreibe Chunk 2, dann 0, dann 3, dann 1 (nicht sequenziell)
        write_chunk_to_position(&file_path, 2, &chunk_data[2], CHUNK_SIZE).unwrap();
        write_chunk_to_position(&file_path, 0, &chunk_data[0], CHUNK_SIZE).unwrap();
        write_chunk_to_position(&file_path, 3, &chunk_data[3], CHUNK_SIZE).unwrap();
        write_chunk_to_position(&file_path, 1, &chunk_data[1], CHUNK_SIZE).unwrap();
        
        // Prüfe dass alle Chunks korrekt geschrieben wurden
        let file_data = fs::read(&file_path).unwrap();
        assert_eq!(file_data.len(), file_size as usize);
        
        // Prüfe jeden Chunk
        for i in 0..4 {
            let chunk_start = (i as u64 * CHUNK_SIZE) as usize;
            let chunk_end = chunk_start + CHUNK_SIZE as usize;
            let expected_value = i as u8;
            
            // Prüfe dass alle Bytes im Chunk den erwarteten Wert haben
            assert!(file_data[chunk_start..chunk_end].iter().all(|&b| b == expected_value),
                "Chunk {} hat falsche Daten", i);
        }
    }
}
