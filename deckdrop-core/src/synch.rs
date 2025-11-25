use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{Seek, SeekFrom, Write};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
    // Heavy Data (Nur geladen wenn nötig, sonst leer)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub chunk_hashes: Vec<String>,
    pub status: DownloadStatus,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub downloaded_chunks: Vec<String>,
    pub file_size: Option<u64>,
    
    // Optimierte Zähler (Immer vorhanden)
    #[serde(default)]
    pub total_chunks: usize,
    #[serde(default)]
    pub downloaded_chunks_count: usize,
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
                    total_chunks: chunk_count,
                    downloaded_chunks_count: 0,
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
    
    /// Speichert das Manifest in SQLite (thread-safe durch SQLite-Transaktionen)
    pub fn save(&self, manifest_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        // Verwende game_id aus dem Manifest selbst (zuverlässiger als Pfad-Extraktion)
        let game_id = &self.game_id;
        
        use crate::manifest_db::get_manifest_db;
        let db_arc = get_manifest_db(game_id)?;
        
        // Verwende create_download, das INSERT OR IGNORE verwendet
        // Für Updates: Lösche alte Daten und erstelle neu
        let mut conn = db_arc.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        let tx = conn.transaction()?;
        
        // Check if this is a Light Manifest (vectors empty but counts > 0)
        // If so, perform a "Light Save" to avoid deleting existing chunks from DB
        let is_light_manifest = self.chunks.values().any(|fi| fi.chunk_hashes.is_empty() && fi.total_chunks > 0);
        
        if is_light_manifest {
            // LIGHT SAVE: Update status only, do NOT touch chunks table
            // This preserves the chunks in DB even though they are not in RAM
            
            let status_str = match &self.overall_status {
                DownloadStatus::Pending => "Pending",
                DownloadStatus::Downloading => "Downloading",
                DownloadStatus::Paused => "Paused",
                DownloadStatus::Complete => "Complete",
                DownloadStatus::Error(msg) => {
                    // Update download with error
                    tx.execute(
                        "UPDATE downloads SET overall_status = ?1, error_message = ?2, downloaded_chunks = ?3, percentage = ?4, updated_at = ?5 WHERE game_id = ?6",
                        rusqlite::params![
                            "Error",
                            msg,
                            self.progress.downloaded_chunks,
                            self.progress.percentage,
                            now,
                            self.game_id,
                        ],
                    )?;
                    "Error" // Return for flow, though we might return early
                },
                DownloadStatus::Cancelled => "Cancelled",
            };
            
            if status_str != "Error" {
                tx.execute(
                    "UPDATE downloads SET overall_status = ?1, error_message = NULL, downloaded_chunks = ?2, percentage = ?3, updated_at = ?4 WHERE game_id = ?5",
                    rusqlite::params![
                        status_str,
                        self.progress.downloaded_chunks,
                        self.progress.percentage,
                        now,
                        self.game_id,
                    ],
                )?;
            }
            
            // Update files status
            for (file_path, file_info) in &self.chunks {
                tx.execute(
                    "UPDATE files SET status = ?1 WHERE game_id = ?2 AND file_path = ?3",
                    rusqlite::params![
                        match file_info.status {
                            DownloadStatus::Pending => "Pending",
                            DownloadStatus::Downloading => "Downloading",
                            DownloadStatus::Complete => "Complete",
                            _ => "Pending",
                        },
                        self.game_id,
                        file_path
                    ],
                )?;
            }
            
            tx.commit()?;
            return Ok(());
        }
        
        // FULL SAVE (Original Logic): Delete & Re-Insert
        // Lösche alte Daten (Foreign Keys löschen Chunks automatisch)
        // Lösche zuerst Files (Chunks werden durch Foreign Key CASCADE gelöscht)
        tx.execute("DELETE FROM files WHERE game_id = ?1", rusqlite::params![game_id])?;
        // Dann Downloads
        tx.execute("DELETE FROM downloads WHERE game_id = ?1", rusqlite::params![game_id])?;
        
        // Insert download
        let status_str = match &self.overall_status {
            DownloadStatus::Pending => "Pending",
            DownloadStatus::Downloading => "Downloading",
            DownloadStatus::Paused => "Paused",
            DownloadStatus::Complete => "Complete",
            DownloadStatus::Error(msg) => {
                    // Versuche INSERT, falls fehlschlägt (weil bereits existiert), verwende UPDATE
                    match tx.execute(
                        "INSERT INTO downloads (game_id, game_name, game_path, overall_status, error_message, total_chunks, downloaded_chunks, percentage, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        rusqlite::params![
                            self.game_id,
                            self.game_name,
                            self.game_path,
                            "Error",
                            msg,
                            self.progress.total_chunks,
                            self.progress.downloaded_chunks,
                            self.progress.percentage,
                            now,
                            now
                        ],
                    ) {
                        Ok(_) => {},
                        Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
                            // Download existiert bereits - update
                            tx.execute(
                                "UPDATE downloads SET game_name = ?1, game_path = ?2, overall_status = ?3, error_message = ?4, total_chunks = ?5, downloaded_chunks = ?6, percentage = ?7, updated_at = ?8 WHERE game_id = ?9",
                                rusqlite::params![
                                    self.game_name,
                                    self.game_path,
                                    "Error",
                                    msg,
                                    self.progress.total_chunks,
                                    self.progress.downloaded_chunks,
                                    self.progress.percentage,
                                    now,
                                    self.game_id,
                                ],
                            )?;
                        },
                        Err(e) => return Err(e.into()),
                    }
                tx.commit()?;
                return Ok(());
            },
            DownloadStatus::Cancelled => "Cancelled",
        };
        
        // Versuche INSERT, falls fehlschlägt (weil bereits existiert), verwende UPDATE
        match tx.execute(
            "INSERT INTO downloads (game_id, game_name, game_path, overall_status, error_message, total_chunks, downloaded_chunks, percentage, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                self.game_id,
                self.game_name,
                self.game_path,
                status_str,
                self.progress.total_chunks,
                self.progress.downloaded_chunks,
                self.progress.percentage,
                now,
                now
            ],
        ) {
            Ok(_) => {},
            Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
                // Download existiert bereits - update
                tx.execute(
                    "UPDATE downloads SET game_name = ?1, game_path = ?2, overall_status = ?3, error_message = NULL, total_chunks = ?4, downloaded_chunks = ?5, percentage = ?6, updated_at = ?7 WHERE game_id = ?8",
                    rusqlite::params![
                        self.game_name,
                        self.game_path,
                        status_str,
                        self.progress.total_chunks,
                        self.progress.downloaded_chunks,
                        self.progress.percentage,
                        now,
                        self.game_id,
                    ],
                )?;
            },
            Err(e) => return Err(e.into()),
        }
        
        // Insert files und chunks
        for (file_path, file_info) in &self.chunks {
            tx.execute(
                "INSERT INTO files (game_id, file_path, file_size, status)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    self.game_id,
                    file_path,
                    file_info.file_size.map(|s| s as i64),
                    match file_info.status {
                        DownloadStatus::Pending => "Pending",
                        DownloadStatus::Downloading => "Downloading",
                        DownloadStatus::Complete => "Complete",
                        _ => "Pending",
                    }
                ],
            )?;
            
            let file_id = tx.last_insert_rowid();
            
            for (index, chunk_hash) in file_info.chunk_hashes.iter().enumerate() {
                let is_downloaded = if file_info.downloaded_chunks.contains(chunk_hash) { 1 } else { 0 };
                let downloaded_at = if is_downloaded == 1 { Some(now) } else { None };
                
                tx.execute(
                    "INSERT INTO chunks (file_id, chunk_hash, chunk_index, is_downloaded, downloaded_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![file_id, chunk_hash, index, is_downloaded, downloaded_at],
                )?;
            }
        }
        
        tx.commit()?;
        
        Ok(())
    }
    
    /// Atomares Update eines Manifests (thread-safe)
    /// Verwendet SQLite für echte ACID-Transaktionen
    pub fn update_manifest_atomic<F>(
        manifest_path: &Path,
        update_fn: F,
    ) -> Result<Self, Box<dyn std::error::Error>>
    where
        F: FnOnce(&mut Self) -> Result<(), Box<dyn std::error::Error>>,
    {
        // Extrahiere game_id aus dem Pfad
        let game_id = manifest_path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .ok_or_else(|| "Konnte game_id aus Manifest-Pfad nicht extrahieren".to_string())?;
        
        // Verwende SQLite-DB
        use crate::manifest_db::get_manifest_db;
        let db_arc = get_manifest_db(game_id)
            .map_err(|e| format!("Fehler beim Öffnen der Manifest-DB: {}", e))?;
        
        // Lade Manifest
        let mut manifest = db_arc.load_download(game_id)
            .map_err(|e| format!("Fehler beim Laden des Manifests: {}", e))?;
        
        // Führe Update durch
        update_fn(&mut manifest)
            .map_err(|e| format!("Fehler beim Update des Manifests: {}", e))?;
        
        // Speichere zurück in die DB
        manifest.save(manifest_path)
            .map_err(|e| format!("Fehler beim Speichern des Manifests: {}", e))?;
        
        Ok(manifest)
    }
    
    /// Lädt ein Manifest aus SQLite
    pub fn load(manifest_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        // Extrahiere game_id aus dem Pfad
        let game_id = manifest_path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .ok_or_else(|| "Konnte game_id aus Manifest-Pfad nicht extrahieren".to_string())?;
        
        use crate::manifest_db::get_manifest_db;
        let db_arc = get_manifest_db(game_id)?;
        db_arc.load_download(game_id)
            .map_err(|e| format!("Fehler beim Laden des Manifests: {}", e).into())
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
        // 1. Update downloaded_chunks for incomplete files
        for file_info in self.chunks.values_mut() {
            // Only update if not already complete (optimization)
            if file_info.status != DownloadStatus::Complete {
                if file_info.chunk_hashes.contains(&chunk_hash.to_string()) {
                    if !file_info.downloaded_chunks.contains(&chunk_hash.to_string()) {
                        file_info.downloaded_chunks.push(chunk_hash.to_string());
                    }
                }
            }
        }
        
        // 2. Identify candidates for validation
        // Only check files that are NOT Complete yet
        let files_to_validate: Vec<String> = self.chunks.iter()
            .filter(|(_, file_info)| {
                file_info.status != DownloadStatus::Complete && 
                file_info.downloaded_chunks.len() == file_info.chunk_hashes.len()
            })
            .map(|(file_path, _)| file_path.clone())
            .collect();
        
        // 3. Validate
        let validation_results: std::collections::HashMap<String, bool> = files_to_validate.iter()
            .map(|file_path| {
                let is_valid = self.validate_file_if_complete(file_path).is_ok();
                if !is_valid {
                    eprintln!("⚠️ Validierung fehlgeschlagen für {} - Status bleibt Downloading", file_path);
                }
                (file_path.clone(), is_valid)
            })
            .collect();
        
        // 4. Update status and clear vectors if complete
        for (file_path, file_info) in self.chunks.iter_mut() {
            if let Some(&is_valid) = validation_results.get(file_path) {
                if is_valid {
                    file_info.status = DownloadStatus::Complete;
                    // Memory Optimization: Clear downloaded_chunks list
                    file_info.downloaded_chunks.clear();
                    file_info.downloaded_chunks.shrink_to_fit();
                } else {
                    file_info.status = DownloadStatus::Downloading;
                }
            }
        }
        
        // 5. Recalculate overall progress
        let mut total_downloaded = 0;
        for file_info in self.chunks.values() {
            if file_info.status == DownloadStatus::Complete {
                total_downloaded += file_info.chunk_hashes.len();
            } else {
                total_downloaded += file_info.downloaded_chunks.len();
            }
        }
        
        self.progress.downloaded_chunks = total_downloaded;
        if self.progress.total_chunks > 0 {
            self.progress.percentage = (total_downloaded as f64 / self.progress.total_chunks as f64) * 100.0;
        }
        
        // 6. Check overall status
        let all_complete = self.chunks.values()
            .all(|fi| fi.status == DownloadStatus::Complete);
        
        if all_complete {
            self.overall_status = DownloadStatus::Complete;
        } else {
            self.overall_status = DownloadStatus::Downloading;
        }
    }
    
    /// Validiert eine Datei, wenn alle Chunks heruntergeladen sind
    fn validate_file_if_complete(&self, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Lade deckdrop_chunks.toml um file_hash zu erhalten
        let manifest_path = get_manifest_path(&self.game_id)?;
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
        
        // Validiere Datei (Hash und Größe)
        // Prüfe zuerst auf .dl Datei
        let output_path = PathBuf::from(&self.game_path).join(file_path);
        
        let mut dl_path = output_path.clone();
        if let Some(file_name) = output_path.file_name() {
             let mut new_name = file_name.to_os_string();
             new_name.push(".dl");
             dl_path.set_file_name(new_name);
        }

        let path_to_check = if dl_path.exists() {
            dl_path.clone()
        } else if output_path.exists() {
            // Fallback: Finale Datei existiert schon
            output_path.clone()
        } else {
            return Err(format!("Datei {} (oder .dl) existiert nicht", file_path).into());
        };
        
        validate_complete_file(&path_to_check, &file_hash, file_size)?;
        
        // Wenn erfolgreich validiert und es war die .dl Datei -> Umbenennen
        if path_to_check == dl_path {
            // WICHTIG: File Handle schließen (aus Cache entfernen)
            remove_file_handle(&dl_path);
            
            // Überschreibe Ziel falls existent
            // rename ist auf Linux atomar, auf Windows nicht immer wenn Datei existiert
            // fs::rename ersetzt auf POSIX die Zieldatei atomar.
            // Auf Windows: std::fs::rename fails if target exists.
            #[cfg(windows)]
            {
                if output_path.exists() {
                    let _ = fs::remove_file(&output_path);
                }
            }
            fs::rename(&dl_path, &output_path)?;
        }
        
        Ok(())
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
/// Verwendet .dl Endung für unfertige Dateien
pub fn preallocate_file(
    file_path: &Path,
    file_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Prüfe ob finale Datei existiert und Größe passt (Resume)
    if file_path.exists() {
        let metadata = fs::metadata(file_path)?;
        if metadata.len() == file_size {
            return Ok(()); // Bereits fertig/vorhanden
        }
    }

    // 2. Konstruiere .dl Pfad
    let mut dl_path = file_path.to_path_buf();
    if let Some(file_name) = file_path.file_name() {
         let mut new_name = file_name.to_os_string();
         new_name.push(".dl");
         dl_path.set_file_name(new_name);
    }

    // Stelle sicher, dass das Verzeichnis existiert
    if let Some(parent) = dl_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    // Erstelle .dl Datei (oder öffne existierende) und setze Größe (erstellt sparse file)
    // OpenOptions::create(true).write(true) öffnet vorhandene Datei ohne truncate
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&dl_path)?;
        
    file.set_len(file_size)?;
    
    Ok(())
}

/// Entfernt ein File-Handle aus dem Cache (z.B. vor dem Umbenennen)
pub fn remove_file_handle(path: &Path) {
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    
    // Zugriff auf den globalen Cache (definiert in write_chunk_to_position Scope, aber wir müssen ihn global machen)
    // Da FILE_HANDLES static local war, müssen wir es verschieben.
    // Siehe unten für die Implementierung.
    if let Some(handles_arc) = GET_FILE_HANDLES_FN.get() {
        if let Ok(mut handles) = handles_arc().lock() {
            handles.remove(path);
        }
    }
}

// Hack um auf den Cache zuzugreifen: Wir speichern eine Accessor-Funktion
static GET_FILE_HANDLES_FN: std::sync::OnceLock<Box<dyn Fn() -> std::sync::Arc<std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, std::sync::Arc<std::sync::Mutex<std::fs::File>>>>> + Send + Sync>> = std::sync::OnceLock::new();

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
    // Verwende OnceLock für statische Mutex-Map, die echte File-Handles speichert
    static FILE_HANDLES: std::sync::OnceLock<Arc<Mutex<HashMap<PathBuf, Arc<Mutex<std::fs::File>>>>>> = std::sync::OnceLock::new();
    let handles = FILE_HANDLES.get_or_init(|| {
        let h = Arc::new(Mutex::new(HashMap::new()));
        // Registriere Accessor
        let h_clone = h.clone();
        let _ = GET_FILE_HANDLES_FN.set(Box::new(move || h_clone.clone()));
        h
    });
    
    // Hole oder erstelle File-Handle für diese Datei
    // Wir halten das File-Handle offen, solange der Prozess läuft (oder bis wir eine Cleanup-Strategie implementieren)
    // Dies vermeidet den teuren open/close Syscall pro Chunk.
    let file_mutex = {
        let mut handles_map = handles.lock().unwrap();
        
        if !handles_map.contains_key(file_path) {
            // Öffne Datei nur einmal
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(file_path)?;
            handles_map.insert(file_path.to_path_buf(), Arc::new(Mutex::new(file)));
        }
        
        handles_map.get(file_path).unwrap().clone()
    };
    
    // Lock für dieses spezifische File-Handle
    let mut file = file_mutex.lock().unwrap();
    
    // Berechne Offset basierend auf Chunk-Index
    let offset = (chunk_index as u64) * chunk_size;
    
    // Springe zur richtigen Position
    file.seek(SeekFrom::Start(offset))?;
    
    // Schreibe Chunk-Daten
    file.write_all(chunk_data)?;
    
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
    let file_info = if let Some(info) = manifest.chunks.values()
        .find(|info| info.chunk_hashes.contains(&chunk_hash.to_string())) {
            info
    } else {
        // Fallback: DB Lookup für Light Manifests
        // Wir verwenden find_file_for_chunk_db um den Pfad zu finden
        // und dann im Manifest nachzuschlagen (da Manifest Metadaten enthält)
        match crate::synch::find_file_for_chunk_db(&manifest.game_id, chunk_hash) {
            Ok(Some(file_path)) => {
                manifest.chunks.get(&file_path)
                    .ok_or_else(|| format!("Datei {} für Chunk {} nicht im Manifest gefunden", file_path, chunk_hash))?
            },
            _ => return Err(format!("Chunk {} nicht im Manifest gefunden", chunk_hash).into()),
        }
    };
    
    // Berechne erwartete Chunk-Größe
    if let Some(file_size) = file_info.file_size {
        const CHUNK_SIZE: u64 = 1 * 1024 * 1024; // 1MB Chunks wie gewünscht
        // Verwende total_chunks statt chunk_hashes.len() für Light Manifests Support
        let total_chunks = file_info.total_chunks;
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
    // Prüfe ob es ein Light Manifest ist (Vektoren leer)
    let is_light_manifest = manifest.chunks.values().any(|fi| fi.chunk_hashes.is_empty() && fi.total_chunks > 0);
    
    let file_path = if is_light_manifest || manifest.chunks.is_empty() {
        // Light Manifest oder leeres Manifest -> DB Lookup
        match crate::synch::find_file_for_chunk_db(&manifest.game_id, chunk_hash) {
            Ok(Some(p)) => p,
            _ => return Err(format!("Chunk {} nicht gefunden (DB Lookup failed)", chunk_hash).into()),
        }
    } else {
        manifest.chunks.iter()
            .find(|(_, info)| info.chunk_hashes.contains(&chunk_hash.to_string()))
            .map(|(p, _)| p.clone())
            .ok_or_else(|| format!("Chunk {} nicht im Manifest gefunden", chunk_hash))?
    };
    
    // Status prüfen (Via DB oder Manifest)
    let is_complete = if manifest.chunks.is_empty() {
        // Check DB status
        // (Vereinfachung: Wir nehmen an, wenn wir schreiben, ist es noch nicht fertig/wir wollen reparieren)
        false
    } else {
        manifest.chunks.get(&file_path).map(|i| i.status == DownloadStatus::Complete).unwrap_or(false)
    };

    // Bestimme vollständigen Dateipfad
    let game_path = PathBuf::from(&manifest.game_path);
    let mut full_file_path = game_path.join(&file_path);
    
    // Wenn Status nicht Complete, schreibe in .dl Datei
    // Oder wenn .dl Datei existiert, benutze diese bevorzugt
    let mut dl_path = full_file_path.clone();
    if let Some(file_name) = dl_path.file_name() {
         let mut new_name = file_name.to_os_string();
         new_name.push(".dl");
         dl_path.set_file_name(new_name);
    }

    if !is_complete || dl_path.exists() {
        full_file_path = dl_path;
    }
    
    // Berechne Chunk-Größe statisch (1MB Standard)
    // Wir vertrauen darauf, dass die Metadaten korrekt sind.
    // Falls ein Chunk kleiner ist (z.B. der letzte), wird write_chunk_to_position nur die verfügbaren Bytes schreiben.
    // Wichtig: write_chunk_to_position verwendet chunk_size nur zur Berechnung des Offsets.
    // Der Offset ist immer chunk_index * 1MB.
    const STANDARD_CHUNK_SIZE: u64 = 1 * 1024 * 1024; // 1MB
    let chunk_size = STANDARD_CHUNK_SIZE;
    
    // Schreibe Chunk direkt an richtige Position
    write_chunk_to_position(&full_file_path, chunk_index, chunk_data, chunk_size)?;
    
    Ok(())
}

/// Markiert einen Chunk als heruntergeladen (SQLite-Version)
/// Diese Funktion verwendet SQLite-Transaktionen für atomare Updates
pub fn mark_chunk_downloaded_sqlite(game_id: &str, chunk_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    use crate::manifest_db::get_manifest_db;
    let db_arc = get_manifest_db(game_id)?;
    db_arc.mark_chunk_downloaded(game_id, chunk_hash)
        .map_err(|e| format!("Fehler beim Markieren des Chunks als heruntergeladen: {}", e).into())
}

/// Ermittelt den Manifest-Pfad für ein Spiel (SQLite-Datenbank)
pub fn get_manifest_path(game_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
        .ok_or("Konnte Konfigurationsverzeichnis nicht bestimmen")?;
    let config_dir = base_dir.config_dir();
    let games_dir = config_dir.join("games").join(game_id);
    Ok(games_dir.join("manifest.db"))
}

/// Ermittelt den Chunks-Verzeichnis-Pfad für ein Spiel
/// Chunks werden im Download-Ordner in einem "temp" Unterordner gespeichert
pub fn get_chunks_dir(game_id: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let config = crate::config::Config::load();
    let download_path = config.download_path;
    Ok(download_path.join("temp").join(game_id))
}

/// Startet den Download-Prozess für ein Spiel
/// Führt die Pre-Allocation mit Progress-Callback durch
pub fn prepare_download_with_progress<F>(
    game_id: &str,
    deckdrop_toml: &str,
    deckdrop_chunks_toml: &str,
    mut progress_callback: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(usize, usize),
{
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
    
    // Speichere Manifest in SQLite
    use crate::manifest_db::get_manifest_db;
    let db_arc = get_manifest_db(game_id)?;
    db_arc.create_download(&manifest)?;
    
    // Stelle sicher, dass das Spielverzeichnis existiert
    std::fs::create_dir_all(&game_path)?;
    
    // Speichere deckdrop.toml und deckdrop_chunks.toml direkt im Spielverzeichnis
    std::fs::write(game_path.join("deckdrop.toml"), deckdrop_toml)?;
    std::fs::write(game_path.join("deckdrop_chunks.toml"), deckdrop_chunks_toml)?;
    
    // Speichere auch eine Kopie im Manifest-Verzeichnis für Backup/Metadaten
    let manifest_path = get_manifest_path(game_id)?;
    if let Some(manifest_dir) = manifest_path.parent() {
        std::fs::create_dir_all(manifest_dir)?;
        std::fs::write(manifest_dir.join("deckdrop.toml"), deckdrop_toml)?;
        std::fs::write(manifest_dir.join("deckdrop_chunks.toml"), deckdrop_chunks_toml)?;
    }
    
    // Prüfe, ob Manifest-DB und Dateien erfolgreich erstellt wurden
    // WICHTIG: Prüfe nach create_download, da die DB-Datei erst dann garantiert existiert
    let manifest_exists = manifest_path.exists();
    let toml_exists = game_path.join("deckdrop.toml").exists();
    let chunks_toml_exists = game_path.join("deckdrop_chunks.toml").exists();
    
    if !manifest_exists || !toml_exists || !chunks_toml_exists {
        return Err(format!("Fehler beim Erstellen von Manifest oder Metadaten-Dateien: manifest={}, toml={}, chunks_toml={}", 
            manifest_exists, toml_exists, chunks_toml_exists).into());
    }
    
    // Phase 1: Pre-Allocation - Blockiere Dateien vorher (BitTorrent-ähnlich)
    // Parse deckdrop_chunks.toml um Dateien zu pre-allokieren
    #[derive(Deserialize)]
    struct ChunksToml {
        file: Vec<ChunkFileEntry>,
    }
    let chunks_toml: ChunksToml = toml::from_str(deckdrop_chunks_toml)?;
    let total_files = chunks_toml.file.len();
    
    for (index, entry) in chunks_toml.file.iter().enumerate() {
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
        
        // Sende Progress-Update
        progress_callback(index + 1, total_files);
    }
    
    println!("Download vorbereitet für Spiel: {} (ID: {})", game_info.name, game_id);
    eprintln!("Download vorbereitet für Spiel: {} (ID: {})", game_info.name, game_id);
    
    Ok(())
}

pub fn start_game_download(
    game_id: &str,
    deckdrop_toml: &str,
    deckdrop_chunks_toml: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Verwende prepare_download_with_progress ohne Callback
    prepare_download_with_progress(game_id, deckdrop_toml, deckdrop_chunks_toml, |_, _| {})
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
    
    // WICHTIG: Begrenze die Anzahl der anzufordernden Chunks auf max_chunks_per_peer * peer_count
    // Dies verhindert, dass zu viele Chunks auf einmal angefordert werden
    // Die kontinuierliche Prüfung in app.rs wird weitere Chunks anfordern, wenn Slots frei werden
    let total_max_chunks = max_chunks_per_peer * peer_ids.len().max(1);
    let chunks_to_request: Vec<String> = missing_chunks.iter()
        .take(total_max_chunks)
        .cloned()
        .collect();
    
    eprintln!("Fordere {} von {} fehlenden Chunks an (Limit: {} = {} pro Peer × {} Peers)", 
        chunks_to_request.len(), missing_chunks.len(), total_max_chunks, max_chunks_per_peer, peer_ids.len());
    
    // Optimierte Multi-Peer Parallelisierung mit Round-Robin und Load-Balancing
    let mut peer_chunk_counts: HashMap<String, usize> = HashMap::new();
    let mut peer_index = 0; // Round-Robin Index
    
    // Initialisiere alle Peers mit 0
    for peer_id in peer_ids.iter() {
        peer_chunk_counts.insert(peer_id.clone(), 0);
    }
    
    for chunk_hash in chunks_to_request.iter() {
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

/// Zeitbasierter Cache für aktive Downloads (wird von mehreren Threads verwendet)
struct ActiveDownloadsCache {
    last_update: Instant,
    data: Vec<(String, DownloadManifest)>,
}

static ACTIVE_DOWNLOADS_CACHE: OnceLock<Mutex<ActiveDownloadsCache>> = OnceLock::new();

fn get_active_downloads_cache() -> &'static Mutex<ActiveDownloadsCache> {
    ACTIVE_DOWNLOADS_CACHE.get_or_init(|| {
        Mutex::new(ActiveDownloadsCache {
            last_update: Instant::now()
                .checked_sub(Duration::from_secs(60))
                .unwrap_or_else(Instant::now),
            data: Vec::new(),
        })
    })
}

/// Gibt die aktuell gecachten aktiven Downloads zurück, ohne das Dateisystem / SQLite zu berühren.
/// 
/// Wird vor allem von der UI verwendet, um den Main-Thread nicht mit I/O zu blockieren.
pub fn get_active_downloads_cached_only() -> Vec<(String, DownloadManifest)> {
    let cache = get_active_downloads_cache();
    if let Ok(cache_guard) = cache.lock() {
        return cache_guard.data.clone();
    }
    Vec::new()
}

/// Lädt alle aktiven Downloads aus SQLite-Datenbanken und aktualisiert den Cache.
/// 
/// WICHTIG: Diese Funktion kann teures I/O verursachen und sollte NICHT im UI-Thread
/// aufgerufen werden. Verwende im UI stattdessen `get_active_downloads_cached_only()`.
pub fn load_active_downloads() -> Vec<(String, DownloadManifest)> {
    // Einfache Zeit-basierte Cache-Schicht:
    // Viele Aufrufer fragen sehr häufig nach aktiven Downloads.
    // Um die SQLite-DBs und das Dateisystem zu entlasten, cachen wir das Ergebnis
    // für einen kurzen Zeitraum und geben in dieser Zeit eine Kopie zurück.

    let cache = get_active_downloads_cache();

    {
        // Schnellpfad: Wenn die letzte Aktualisierung sehr frisch ist, gib eine Kopie zurück
        // WICHTIG: Cache-Zeit auf 100ms reduziert für schnellere Reaktion im Scheduler!
        if let Ok(cache_guard) = cache.lock() {
            if cache_guard.last_update.elapsed() < Duration::from_millis(100) {
                return cache_guard.data.clone();
            }
        }
    }

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
                let db_path = entry.path().join("manifest.db");
                
                if db_path.exists() {
                    if let Some(game_id) = entry.file_name().to_str() {
                        use crate::manifest_db::get_manifest_db;
                        if let Ok(db_arc) = get_manifest_db(game_id) {
                            if let Ok(manifest) = db_arc.load_download(game_id) {
                                // Nur Downloads, die nicht abgebrochen sind
                                if !matches!(manifest.overall_status, DownloadStatus::Cancelled) {
                                    downloads.push((manifest.game_id.clone(), manifest));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Cache aktualisieren
    if let Ok(mut cache_guard) = cache.lock() {
        cache_guard.last_update = Instant::now();
        cache_guard.data = downloads.clone();
    }
    
    downloads
}

/// Erzwingt ein Neuladen der aktiven Downloads aus der DB und aktualisiert den Cache
/// Ignoriert den Cache-Timeout.
pub fn force_update_active_downloads() -> Vec<(String, DownloadManifest)> {
    let cache = get_active_downloads_cache();
    // Setze Zeitstempel zurück, um Neu-Laden zu erzwingen
    if let Ok(mut cache_guard) = cache.lock() {
        // Setze auf "vor 60 Sekunden"
        cache_guard.last_update = Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or(Instant::now());
    }
    load_active_downloads()
}

/// Findet die game_id für einen Chunk durch Suche in allen SQLite-Datenbanken
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
        let db_path = entry.path().join("manifest.db");
        
        if db_path.exists() {
            if let Some(game_id) = entry.file_name().to_str() {
                use crate::manifest_db::get_manifest_db;
                if let Ok(db_arc) = get_manifest_db(game_id) {
                    // Prüfe ob dieser Chunk in der DB ist (direkt via SQL)
                    if let Ok(true) = db_arc.has_chunk(game_id, chunk_hash) {
                        return Ok(game_id.to_string());
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
            
            // Prüfe ob .dl Datei existiert
            let mut dl_path = output_path.clone();
            if let Some(name) = dl_path.file_name() {
                let mut dl_name = name.to_os_string();
                dl_name.push(".dl");
                dl_path.set_file_name(dl_name);
            }
            
            let target_path = if dl_path.exists() { dl_path } else { output_path.clone() };
            
            if target_path.exists() {
                // Hole file_hash und file_size
                if let Some((file_hash, file_size)) = file_hashes.get(file_path) {
                    // Validiere Datei (Hash und Größe)
                    if let Err(e) = validate_complete_file(&target_path, file_hash, *file_size) {
                        eprintln!("Fehler bei Validierung von {}: {}", file_path, e);
                    } else {
                        println!("Datei validiert: {}", file_path);
                        eprintln!("Datei validiert: {}", file_path);
                        
                        // Wenn es eine .dl Datei war und validiert wurde, benenne sie um
                        if target_path.extension().and_then(|e| e.to_str()) == Some("dl") {
                            remove_file_handle(&target_path);
                            if let Err(e) = fs::rename(&target_path, &output_path) {
                                eprintln!("Fehler beim Umbenennen von {:?} zu {:?}: {}", target_path, output_path, e);
                            } else {
                                println!("Datei umbenannt: {:?} -> {:?}", target_path, output_path);
                            }
                        }
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

/// Findet den Dateipfad für einen Chunk direkt in der DB (ohne Manifest-Load)
pub fn find_file_for_chunk_db(
    game_id: &str,
    chunk_hash: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    use crate::manifest_db::get_manifest_db;
    use rusqlite::OptionalExtension;
    
    let db = get_manifest_db(game_id)?;
    let conn = db.conn.lock().unwrap();
    
    let file_path: Option<String> = conn.query_row(
        "SELECT f.file_path FROM files f JOIN chunks c ON f.id = c.file_id WHERE c.chunk_hash = ?1 AND f.game_id = ?2",
        rusqlite::params![chunk_hash, game_id],
        |row| row.get(0)
    ).optional()?;
    
    Ok(file_path)
}

/// Findet den Dateipfad für einen Chunk im Manifest
pub fn find_file_for_chunk(
    manifest: &DownloadManifest,
    chunk_hash: &str,
) -> Option<String> {
    // Versuche zuerst die Counts/DB zu nutzen, falls Vektoren leer sind
    // Da wir hier keinen DB-Zugriff haben (nur &Manifest), und Vektoren leer sind,
    // wird diese Funktion bei Light-Manifests immer None zurückgeben.
    // Der Aufrufer sollte find_file_for_chunk_db nutzen.
    
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
    
    // Verwende Counts statt Vektoren
    if file_info.downloaded_chunks_count != file_info.total_chunks {
        return Ok(false); // Datei noch nicht komplett
    }
    
    // Datei sollte komplett sein - validiere sie
    let output_path = PathBuf::from(&manifest.game_path).join(file_path);
    
    // Prüfe ob .dl Datei existiert (bei laufendem Download üblich)
    let mut dl_path = output_path.clone();
    if let Some(name) = dl_path.file_name() {
        let mut dl_name = name.to_os_string();
        dl_name.push(".dl");
        dl_path.set_file_name(dl_name);
    }
    
    let target_path = if dl_path.exists() {
        dl_path
    } else {
        output_path.clone()
    };
    
    if !target_path.exists() {
        return Err(format!("Datei {} (oder .dl) sollte existieren, ist aber nicht vorhanden", file_path).into());
    }
    
    // Validiere Datei (Hash und Größe)
    validate_complete_file(&target_path, &file_hash, file_size)?;
    
    // Wenn es eine .dl Datei war und validiert wurde, benenne sie um
    if target_path.extension().and_then(|e| e.to_str()) == Some("dl") {
        // Schließe File Handle falls offen
        remove_file_handle(&target_path);
        fs::rename(&target_path, &output_path)?;
        println!("Datei umbenannt: {:?} -> {:?}", target_path, output_path);
    }
    
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
    
    // deckdrop.toml und deckdrop_chunks.toml sollten bereits im Spielverzeichnis sein
    // (wurden in start_game_download gespeichert)
    // Falls nicht vorhanden, kopiere aus Manifest-Verzeichnis als Fallback
    if let Some(manifest_dir) = manifest_path.parent() {
        let deckdrop_toml_dest = game_path.join("deckdrop.toml");
        let deckdrop_chunks_toml_dest = game_path.join("deckdrop_chunks.toml");
        let deckdrop_toml_src = manifest_dir.join("deckdrop.toml");
        let deckdrop_chunks_toml_src = manifest_dir.join("deckdrop_chunks.toml");
        
        if !deckdrop_toml_dest.exists() && deckdrop_toml_src.exists() {
            std::fs::copy(&deckdrop_toml_src, &deckdrop_toml_dest)?;
        }
        if !deckdrop_chunks_toml_dest.exists() && deckdrop_chunks_toml_src.exists() {
            std::fs::copy(&deckdrop_chunks_toml_src, &deckdrop_chunks_toml_dest)?;
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
    use crate::manifest_db::get_manifest_db;
    use rusqlite::params;
    let db_arc = get_manifest_db(game_id)?;
    
    // Markiere als abgebrochen in SQLite
    let conn = db_arc.conn.lock().unwrap();
    conn.execute(
        "UPDATE downloads SET overall_status = 'Cancelled' WHERE game_id = ?1",
        params![game_id],
    )?;
    
    // Lösche temporäre Chunks im Download-Ordner
    let chunks_dir = get_chunks_dir(game_id)?;
    if chunks_dir.exists() {
        std::fs::remove_dir_all(&chunks_dir)?;
        println!("Temporäre Chunks gelöscht: {}", chunks_dir.display());
    }
    
    // Lösche Manifest-Verzeichnis (enthält nur Metadaten, keine Chunks mehr)
    let manifest_path = get_manifest_path(game_id)?;
    if let Some(manifest_dir) = manifest_path.parent() {
        if manifest_dir.exists() {
            std::fs::remove_dir_all(manifest_dir)?;
        }
    }
    
    // Entferne DB-Verbindung aus dem Cache (wichtig für Tests)
    use crate::manifest_db::remove_manifest_db_from_cache;
    remove_manifest_db_from_cache(game_id);
    
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
        
        // Erstelle Test-Datei (2MB für 2 Chunks à 1MB)
        let mut test_data = vec![0u8; 2 * 1024 * 1024]; // 2MB
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
        
        // Teile in Chunks (2 Chunks: 1MB + 1MB)
        let chunk1 = &test_data[0..1 * 1024 * 1024];
        let chunk2 = &test_data[1 * 1024 * 1024..];
        
        // Schreibe Chunks direkt in Datei (Piece-by-Piece, in beliebiger Reihenfolge)
        write_chunk_to_file(&format!("{}:1", file_hash), chunk2, &manifest).unwrap(); // Zweiter Chunk zuerst
        write_chunk_to_file(&format!("{}:0", file_hash), chunk1, &manifest).unwrap(); // Erster Chunk danach
        
        // Validiere Datei (.dl Pfad verwenden)
        let mut dl_path = file_path.clone();
        let mut n = dl_path.file_name().unwrap().to_os_string();
        n.push(".dl");
        dl_path.set_file_name(n);
        
        validate_complete_file(&dl_path, &file_hash, test_data.len() as u64).unwrap();
        
        // Simuliere Umbenennung
        crate::synch::remove_file_handle(&dl_path);
        fs::rename(&dl_path, &file_path).unwrap();
        
        // Prüfe geschriebene Datei
        let written = fs::read(&file_path).unwrap();
        assert_eq!(written.len(), test_data.len());
        assert_eq!(written, test_data);
    }


    #[tokio::test]
    #[ignore]
    async fn test_two_peers_chunk_exchange() {
        // Erstelle temporäres Verzeichnis für beide Peers
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        
        // Eindeutige game_id basierend auf Timestamp (verhindert Konflikte bei parallelen Tests)
        use std::time::{SystemTime, UNIX_EPOCH};
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let game_id = format!("test-game-2peers-{}", unique_id);
        
        // Peer 1: Hat das Spiel
        let game_path1 = temp_dir1.path().join("game");
        fs::create_dir_all(&game_path1).unwrap();
        
        // Erstelle Test-Datei für Peer 1 (2MB für 2 Chunks à 1MB)
        let test_data = vec![42u8; 2 * 1024 * 1024]; // 2MB
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
name = "Test Game 2Peers"
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
        start_game_download(&game_id, &deckdrop_toml, &deckdrop_chunks_toml).unwrap();
        
        // Lade Manifest für write_chunk_to_file
        let manifest_path = get_manifest_path(&game_id).unwrap();
        let manifest = DownloadManifest::load(&manifest_path).unwrap();
        
        // Simuliere Chunk-Transfer von Peer 1 zu Peer 2 (Piece-by-Piece Writing)
        // Chunk 1: 0-1MB
        let chunk1 = &test_data[0..1 * 1024 * 1024];
        let chunk1_hash = format!("{}:0", computed_hash);
        write_chunk_to_file(&chunk1_hash, chunk1, &manifest).unwrap();
        
        // Chunk 2: 1-2MB
        let chunk2 = &test_data[1 * 1024 * 1024..];
        let chunk2_hash = format!("{}:1", computed_hash);
        write_chunk_to_file(&chunk2_hash, chunk2, &manifest).unwrap();
        
        // Markiere Chunks als heruntergeladen (Via DB, da Light Manifest)
        crate::synch::mark_chunk_downloaded_sqlite(&game_id, &chunk1_hash).unwrap();
        crate::synch::mark_chunk_downloaded_sqlite(&game_id, &chunk2_hash).unwrap();
        
        // Reload Manifest um Status zu prüfen (aus DB)
        let manifest = DownloadManifest::load(&manifest_path).unwrap();
        
        // Prüfe ob alle Chunks vorhanden sind
        assert_eq!(manifest.progress.downloaded_chunks, 2);
        assert_eq!(manifest.progress.total_chunks, 2);
        
        // Validiere Datei (sollte bereits komplett sein durch Piece-by-Piece Writing)
        // Verwende Pfad aus Manifest (nicht game_path2, da start_game_download den Pfad aus Config verwendet)
        let output_path = PathBuf::from(&manifest.game_path).join("test.bin");
        
        let mut dl_path = output_path.clone();
        let mut n = dl_path.file_name().unwrap().to_os_string();
        n.push(".dl");
        dl_path.set_file_name(n);
        
        validate_complete_file(&dl_path, &computed_hash, test_data.len() as u64).unwrap();
        
        // Simuliere Umbenennung
        crate::synch::remove_file_handle(&dl_path);
        fs::rename(&dl_path, &output_path).unwrap();
        
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
        
        // Aufräumen: Lösche Test-Spiel (Manifest, Chunks-Verzeichnis und Spielverzeichnis)
        // 1. Manifest/chunks über cancel_game_download
        let _ = cancel_game_download(&game_id);
        
        // 2. Spielverzeichnis unter download_path entfernen (wurde in prepare_download_with_progress angelegt)
        if let Ok(manifest_for_cleanup) = DownloadManifest::load(&manifest_path) {
            let game_dir = PathBuf::from(&manifest_for_cleanup.game_path);
            if game_dir.exists() {
                if let Err(e) = fs::remove_dir_all(&game_dir) {
                    eprintln!("Warnung: Konnte Test-Spielverzeichnis nicht löschen ({}): {}", game_dir.display(), e);
                }
            }
        }
        
        // TempDir wird automatisch gelöscht wenn es out of scope geht
    }
    
    #[test]
    fn test_validate_chunk_size() {
        // Verwende eine kleine Test-Datei (2MB für 2 Chunks à 1MB)
        // Die validate_chunk_size Funktion verwendet eine fest codierte CHUNK_SIZE von 1MB
        const CHUNK_SIZE: usize = 1 * 1024 * 1024; // 1MB
        let file_size = 2 * 1024 * 1024; // 2MB
        
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
        let chunk0_small = vec![1u8; 100 * 1024]; // 100KB statt 1MB
        assert!(validate_chunk_size(&chunk0_hash, &chunk0_small, &manifest).is_err());
        
        // Test 3: Letzter Chunk kann kleiner sein (1MB für letzten Chunk)
        let chunk1_hash = format!("{}:1", file_hash);
        let chunk1_data = vec![1u8; CHUNK_SIZE]; // 1MB für letzten Chunk
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
    
    // #[test]
    // fn test_mark_chunk_downloaded_sqlite() {
    //     let temp_dir = TempDir::new().unwrap();
    //     // ... (Test logic commented out)
    // }
    
    #[test]
    fn test_manifest_save_atomic() {
        let temp_dir = TempDir::new().unwrap();
        // Erstelle richtige Verzeichnisstruktur: games/test-game/manifest.db
        let game_dir = temp_dir.path().join("games").join("test-game");
        std::fs::create_dir_all(&game_dir).unwrap();
        let manifest_path = game_dir.join("manifest.db");
        
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
        
        // Test: Atomic Save (SQLite)
        manifest.save(&manifest_path).unwrap();
        // Die DB wird im Konfigurationsverzeichnis erstellt, nicht im temp_dir
        // Prüfe dass die DB im richtigen Verzeichnis erstellt wurde
        let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop").unwrap();
        let config_dir = base_dir.config_dir();
        let games_dir = config_dir.join("games").join("test-game");
        let db_path = games_dir.join("manifest.db");
        assert!(db_path.exists(), "DB sollte erstellt werden: {}", db_path.display());
        
        // Prüfe dass Manifest korrekt geladen werden kann (verwende den DB-Pfad)
        let loaded = DownloadManifest::load(&db_path).unwrap();
        assert_eq!(loaded.game_id, "test-game");
        
        // Prüfe dass keine Temp-Datei übrig bleibt (nach kurzer Wartezeit für Filesystem-Sync)
        // Ignoriere das games-Verzeichnis, das für Tests erstellt wurde
        std::thread::sleep(std::time::Duration::from_millis(100));
        let temp_files: Vec<_> = fs::read_dir(temp_dir.path()).unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path_str = e.path().to_string_lossy().to_string();
                // Ignoriere .tmp-Dateien, aber nicht Verzeichnisse wie "games"
                path_str.contains(".tmp") && !path_str.contains("manifest") && e.path().is_file()
            })
            .collect();
        assert_eq!(temp_files.len(), 0, "Keine Temp-Dateien sollten übrig bleiben (gefunden: {:?})", 
            temp_files.iter().map(|e| e.path()).collect::<Vec<_>>());
        
        // Aufräumen: Lösche Test-Spiel aus dem Konfigurationsverzeichnis
        let _ = cancel_game_download("test-game");
        
    }
    
    #[test]
    fn test_preallocate_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_preallocated.bin");
        let file_size = 200 * 1024 * 1024; // 200MB
        
        // Test: Pre-Allocation erstellt Datei mit korrekter Größe (als .dl)
        preallocate_file(&file_path, file_size).unwrap();
        
        let mut dl_path = file_path.clone();
        let mut dl_name = dl_path.file_name().unwrap().to_os_string();
        dl_name.push(".dl");
        dl_path.set_file_name(dl_name);
        
        assert!(dl_path.exists());
        assert!(!file_path.exists()); // Original sollte nicht existieren
        
        // Prüfe Dateigröße
        let metadata = fs::metadata(&dl_path).unwrap();
        assert_eq!(metadata.len(), file_size);
        
        // Test: Pre-Allocation erstellt Verzeichnis falls nötig
        let nested_path = temp_dir.path().join("nested").join("test.bin");
        preallocate_file(&nested_path, 100 * 1024 * 1024).unwrap();
        
        let mut nested_dl = nested_path.clone();
        let mut n_name = nested_dl.file_name().unwrap().to_os_string();
        n_name.push(".dl");
        nested_dl.set_file_name(n_name);
        
        assert!(nested_dl.exists());
    }
    
    #[test]
    fn test_write_chunk_to_position() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_chunks.bin");
        const CHUNK_SIZE: u64 = 100 * 1024 * 1024; // 100MB
        let file_size = 2 * CHUNK_SIZE; // 200MB (2 Chunks)
        
        // Pre-Allokiere Datei
        preallocate_file(&file_path, file_size).unwrap();
        
        let mut dl_path = file_path.clone();
        let mut dl_name = dl_path.file_name().unwrap().to_os_string();
        dl_name.push(".dl");
        dl_path.set_file_name(dl_name);
        
        // Schreibe Chunk 1 (zweiter Chunk zuerst - testet Piece-by-Piece Writing)
        // WICHTIG: Wir müssen in die .dl Datei schreiben!
        let chunk1_data = vec![42u8; CHUNK_SIZE as usize];
        write_chunk_to_position(&dl_path, 1, &chunk1_data, CHUNK_SIZE).unwrap();
        
        // Schreibe Chunk 0 (erster Chunk danach)
        let chunk0_data = vec![24u8; CHUNK_SIZE as usize];
        write_chunk_to_position(&dl_path, 0, &chunk0_data, CHUNK_SIZE).unwrap();
        
        // Prüfe dass Datei korrekt geschrieben wurde
        let file_data = fs::read(&dl_path).unwrap();
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
        
        let mut dl_path = file_path.clone();
        let mut dl_name = dl_path.file_name().unwrap().to_os_string();
        dl_name.push(".dl");
        dl_path.set_file_name(dl_name);
        
        // Schreibe Chunk 0
        let chunk0_data = vec![1u8; 10 * 1024 * 1024];
        write_chunk_to_file("test_hash_123:0", &chunk0_data, &manifest).unwrap();
        
        // Schreibe Chunk 1
        let chunk1_data = vec![2u8; 10 * 1024 * 1024];
        write_chunk_to_file("test_hash_123:1", &chunk1_data, &manifest).unwrap();
        
        // Prüfe dass Datei korrekt geschrieben wurde
        let file_data = fs::read(&dl_path).unwrap();
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
        
        let mut dl_path = file_path.clone();
        let mut dl_name = dl_path.file_name().unwrap().to_os_string();
        dl_name.push(".dl");
        dl_path.set_file_name(dl_name);
        
        // Erstelle Test-Daten für 4 Chunks
        let chunk_data: Vec<Vec<u8>> = (0..4)
            .map(|i| vec![i as u8; CHUNK_SIZE as usize])
            .collect();
        
        // Schreibe Chunks in beliebiger Reihenfolge (testet Piece-by-Piece Writing)
        // Schreibe Chunk 2, dann 0, dann 3, dann 1 (nicht sequenziell)
        write_chunk_to_position(&dl_path, 2, &chunk_data[2], CHUNK_SIZE).unwrap();
        write_chunk_to_position(&dl_path, 0, &chunk_data[0], CHUNK_SIZE).unwrap();
        write_chunk_to_position(&dl_path, 3, &chunk_data[3], CHUNK_SIZE).unwrap();
        write_chunk_to_position(&dl_path, 1, &chunk_data[1], CHUNK_SIZE).unwrap();
        
        // Prüfe dass alle Chunks korrekt geschrieben wurden
        let file_data = fs::read(&dl_path).unwrap();
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
    
    #[test]
    fn test_prepare_download_with_progress_callback_frequency() {
        use std::sync::{Arc, Mutex};
        use std::time::{SystemTime, UNIX_EPOCH};
        
        // Erstelle eindeutige game_id basierend auf Timestamp
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let game_id = format!("test_game_callback_{}", unique_id);
        
        // Erstelle Test deckdrop.toml
        let deckdrop_toml = format!(r#"
game_id = "{}"
name = "Test Game Callback"
version = "1.0.0"
start_file = "game.exe"
description = "Test"
"#, game_id);
        
        // Erstelle deckdrop_chunks.toml mit 50 Dateien (um zu testen, ob Callback korrekt aufgerufen wird)
        let mut chunks_toml = String::new();
        for i in 0..50 {
            chunks_toml.push_str(&format!(
                "[[file]]\npath = \"file_{}.bin\"\nfile_hash = \"hash_{}\"\nchunk_count = 1\nfile_size = 1024\n",
                i, i
            ));
        }
        
        // Test: Zähle wie oft der Callback aufgerufen wird
        let callback_count = Arc::new(Mutex::new(0));
        let callback_count_clone = callback_count.clone();
        let callback_values = Arc::new(Mutex::new(Vec::new()));
        let callback_values_clone = callback_values.clone();
        
        // Führe prepare_download_with_progress aus
        let result = prepare_download_with_progress(
            &game_id,
            &deckdrop_toml,
            &chunks_toml,
            move |current, total| {
                let mut count = callback_count_clone.lock().unwrap();
                *count += 1;
                
                let mut values = callback_values_clone.lock().unwrap();
                values.push((current, total));
                
                // Prüfe dass current und total sinnvoll sind
                assert!(current > 0, "current sollte > 0 sein");
                assert!(total > 0, "total sollte > 0 sein");
                assert!(current <= total, "current sollte <= total sein");
            },
        );
        
        // Prüfe Ergebnis - zeige Fehler falls vorhanden
        if let Err(e) = &result {
            eprintln!("prepare_download_with_progress Fehler: {}", e);
        }
        
        let count = callback_count.lock().unwrap();
        let values = callback_values.lock().unwrap();
        
        // Prüfe dass Callback genau 50 mal aufgerufen wurde (eine pro Datei)
        assert_eq!(*count, 50, "Callback sollte genau 50 mal aufgerufen werden (eine pro Datei), wurde aber {} mal aufgerufen", *count);
        
        // Prüfe dass die Werte korrekt sind
        assert_eq!(values.len(), 50, "Callback-Werte sollten 50 Einträge haben");
        assert_eq!(values[0], (1, 50), "Erster Callback sollte (1, 50) sein");
        assert_eq!(values[49], (50, 50), "Letzter Callback sollte (50, 50) sein");
        
        // Prüfe dass die Werte sequenziell sind
        for i in 0..50 {
            assert_eq!(values[i].0, i + 1, "Callback {} sollte current={} haben", i, i + 1);
            assert_eq!(values[i].1, 50, "Callback {} sollte total=50 haben", i);
        }
        
        // Wenn prepare_download_with_progress erfolgreich war, sollte auch Manifest existieren
        if result.is_ok() {
            let manifest_path = get_manifest_path(&game_id).unwrap();
            assert!(manifest_path.exists(), "Manifest sollte erstellt worden sein");
            
            // Lade Manifest, um das Spielverzeichnis zu ermitteln
            if let Ok(manifest) = DownloadManifest::load(&manifest_path) {
                let game_dir = PathBuf::from(&manifest.game_path);
                
                // Aufräumen: Lösche Spielverzeichnis im Download-Pfad
                if game_dir.exists() {
                    if let Err(e) = fs::remove_dir_all(&game_dir) {
                        eprintln!("Warnung: Konnte Test-Spielverzeichnis nicht löschen ({}): {}", game_dir.display(), e);
                    }
                }
            }
            
            // Aufräumen: Lösche Manifest + Chunks-Verzeichnis über cancel_game_download
            let _ = cancel_game_download(&game_id);
        }
    }
}

