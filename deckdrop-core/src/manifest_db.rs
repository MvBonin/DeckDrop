use rusqlite::{Connection, params, Result as SqliteResult};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use crate::synch::{DownloadManifest, DownloadStatus, DownloadProgress, FileChunkInfo};

/// SQLite-basierte Manifest-Datenbank
/// Thread-safe durch Arc<Mutex<Connection>>
/// Die Connection ist bereits in einem Mutex, daher ist ManifestDB selbst thread-safe
pub struct ManifestDB {
    pub(crate) conn: Arc<Mutex<Connection>>,
}

impl ManifestDB {
    /// Öffnet oder erstellt eine Manifest-Datenbank
    pub fn open(db_path: &Path) -> SqliteResult<Self> {
        // Stelle sicher, dass das Verzeichnis existiert
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                    Some(format!("Konnte Verzeichnis nicht erstellen: {}", e))
                ))?;
        }
        
        let conn = Connection::open(db_path)?;
        
        // Setze Busy Timeout auf 5 Sekunden, um "database is locked" Fehler bei parallelem Zugriff zu vermeiden
        conn.busy_timeout(std::time::Duration::from_secs(30))?;
        
        // Aktiviere WAL-Modus für bessere Performance und parallele Zugriffe
        // PRAGMA journal_mode gibt einen Wert zurück, daher query_row verwenden
        let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        conn.execute("PRAGMA synchronous=NORMAL", [])?;
        conn.execute("PRAGMA foreign_keys=ON", [])?;
        
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        
        db.create_tables()?;
        
        Ok(db)
    }
    
    /// Erstellt die Tabellen, falls sie nicht existieren
    fn create_tables(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        
        // Downloads Tabelle
        conn.execute(
            "CREATE TABLE IF NOT EXISTS downloads (
                game_id TEXT PRIMARY KEY,
                game_name TEXT NOT NULL,
                game_path TEXT NOT NULL,
                overall_status TEXT NOT NULL,
                error_message TEXT,
                total_chunks INTEGER NOT NULL,
                downloaded_chunks INTEGER NOT NULL DEFAULT 0,
                percentage REAL NOT NULL DEFAULT 0.0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        
        // Files Tabelle
        conn.execute(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                game_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_size INTEGER,
                status TEXT NOT NULL,
                FOREIGN KEY (game_id) REFERENCES downloads(game_id) ON DELETE CASCADE,
                UNIQUE(game_id, file_path)
            )",
            [],
        )?;
        
        // Chunks Tabelle
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_id INTEGER NOT NULL,
                chunk_hash TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                is_downloaded INTEGER NOT NULL DEFAULT 0,
                downloaded_at INTEGER,
                FOREIGN KEY (file_id) REFERENCES files(id) ON DELETE CASCADE,
                UNIQUE(file_id, chunk_hash),
                UNIQUE(file_id, chunk_index)
            )",
            [],
        )?;
        
        // Indizes für Performance
        conn.execute("CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_chunks_downloaded ON chunks(file_id, is_downloaded)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_files_game_id ON files(game_id)", [])?;
        
        Ok(())
    }
    
    /// Erstellt ein neues Download-Manifest
    pub fn create_download(&self, manifest: &DownloadManifest) -> SqliteResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        let tx = conn.transaction()?;
        
        // Insert download
        let status_str = match &manifest.overall_status {
            DownloadStatus::Pending => "Pending",
            DownloadStatus::Downloading => "Downloading",
            DownloadStatus::Paused => "Paused",
            DownloadStatus::Complete => "Complete",
            DownloadStatus::Error(msg) => {
                // Versuche INSERT, falls fehlschlägt (weil bereits existiert), verwende UPDATE
                match tx.execute(
                    "INSERT INTO downloads (game_id, game_name, game_path, overall_status, error_message, total_chunks, downloaded_chunks, percentage, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        manifest.game_id,
                        manifest.game_name,
                        manifest.game_path,
                        "Error",
                        msg,
                        manifest.progress.total_chunks,
                        manifest.progress.downloaded_chunks,
                        manifest.progress.percentage,
                        now,
                        now
                    ],
                ) {
                    Ok(_) => {},
                    Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
                        // Download existiert bereits - update
                        tx.execute(
                            "UPDATE downloads SET game_name = ?1, game_path = ?2, overall_status = ?3, error_message = ?4, total_chunks = ?5, downloaded_chunks = ?6, percentage = ?7, updated_at = ?8 WHERE game_id = ?9",
                            params![
                                manifest.game_name,
                                manifest.game_path,
                                "Error",
                                msg,
                                manifest.progress.total_chunks,
                                manifest.progress.downloaded_chunks,
                                manifest.progress.percentage,
                                now,
                                manifest.game_id,
                            ],
                        )?;
                    },
                    Err(e) => return Err(e),
                }
                return tx.commit().map(|_| ());
            },
            DownloadStatus::Cancelled => "Cancelled",
        };
        
        // Versuche INSERT, falls fehlschlägt (weil bereits existiert), verwende UPDATE
        match tx.execute(
            "INSERT INTO downloads (game_id, game_name, game_path, overall_status, error_message, total_chunks, downloaded_chunks, percentage, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9)",
            params![
                manifest.game_id,
                manifest.game_name,
                manifest.game_path,
                status_str,
                manifest.progress.total_chunks,
                manifest.progress.downloaded_chunks,
                manifest.progress.percentage,
                now,
                now
            ],
        ) {
            Ok(_) => {},
            Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::ConstraintViolation => {
                // Download existiert bereits - update
                tx.execute(
                    "UPDATE downloads SET game_name = ?1, game_path = ?2, overall_status = ?3, error_message = NULL, total_chunks = ?4, downloaded_chunks = ?5, percentage = ?6, updated_at = ?7 WHERE game_id = ?8",
                    params![
                        manifest.game_name,
                        manifest.game_path,
                        status_str,
                        manifest.progress.total_chunks,
                        manifest.progress.downloaded_chunks,
                        manifest.progress.percentage,
                        now,
                        manifest.game_id,
                    ],
                )?;
            },
            Err(e) => return Err(e),
        }
        
        // Insert files und chunks
        for (file_path, file_info) in &manifest.chunks {
            tx.execute(
                "INSERT INTO files (game_id, file_path, file_size, status)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    manifest.game_id,
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
                    params![file_id, chunk_hash, index, is_downloaded, downloaded_at],
                )?;
            }
        }
        
        tx.commit()?;
        Ok(())
    }
    
    /// Lädt ein Download-Manifest
    pub fn load_download(&self, game_id: &str) -> SqliteResult<DownloadManifest> {
        let conn = self.conn.lock().unwrap();
        
        // Lade Download-Info
        let mut stmt = conn.prepare(
            "SELECT game_id, game_name, game_path, overall_status, error_message, total_chunks, downloaded_chunks, percentage
             FROM downloads WHERE game_id = ?1"
        )?;
        
        let download_row = stmt.query_row(params![game_id], |row| {
            Ok((
                row.get::<_, String>(0)?,  // game_id
                row.get::<_, String>(1)?,  // game_name
                row.get::<_, String>(2)?,  // game_path
                row.get::<_, String>(3)?,  // overall_status
                row.get::<_, Option<String>>(4)?,  // error_message
                row.get::<_, i64>(5)?,  // total_chunks
                row.get::<_, i64>(6)?,  // downloaded_chunks
                row.get::<_, f64>(7)?,  // percentage
            ))
        })?;
        
        let (_, game_name, game_path, status_str, error_msg, total_chunks, downloaded_chunks, percentage) = download_row;
        
        let overall_status = match status_str.as_str() {
            "Pending" => DownloadStatus::Pending,
            "Downloading" => DownloadStatus::Downloading,
            "Paused" => DownloadStatus::Paused,
            "Complete" => DownloadStatus::Complete,
            "Error" => DownloadStatus::Error(error_msg.unwrap_or_default()),
            "Cancelled" => DownloadStatus::Cancelled,
            _ => DownloadStatus::Pending,
        };
        
        // Lade Files und Chunks
        let mut files_stmt = conn.prepare(
            "SELECT id, file_path, file_size, status FROM files WHERE game_id = ?1"
        )?;
        
        let mut chunks = HashMap::new();
        
        let files_iter = files_stmt.query_map(params![game_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,  // id
                row.get::<_, String>(1)?,  // file_path
                row.get::<_, Option<i64>>(2)?,  // file_size
                row.get::<_, String>(3)?,  // status
            ))
        })?;
        
        for file_row in files_iter {
            let (file_id, file_path, file_size, status_str) = file_row?;
            
            // Lade Chunks für diese Datei
            let mut chunks_stmt = conn.prepare(
                "SELECT chunk_hash, chunk_index, is_downloaded FROM chunks WHERE file_id = ?1 ORDER BY chunk_index"
            )?;
            
            let mut chunk_hashes = Vec::new();
            let mut downloaded_chunks_list = Vec::new();
            
            let chunks_iter = chunks_stmt.query_map(params![file_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,  // chunk_hash
                    row.get::<_, i64>(1)?,  // chunk_index
                    row.get::<_, i64>(2)?,  // is_downloaded
                ))
            })?;
            
            for chunk_row in chunks_iter {
                let (chunk_hash, _, is_downloaded) = chunk_row?;
                chunk_hashes.push(chunk_hash.clone());
                if is_downloaded == 1 {
                    downloaded_chunks_list.push(chunk_hash);
                }
            }
            
            let file_status = match status_str.as_str() {
                "Pending" => DownloadStatus::Pending,
                "Downloading" => DownloadStatus::Downloading,
                "Complete" => DownloadStatus::Complete,
                _ => DownloadStatus::Pending,
            };
            
            chunks.insert(file_path, FileChunkInfo {
                chunk_hashes,
                status: file_status,
                downloaded_chunks: downloaded_chunks_list,
                file_size: file_size.map(|s| s as u64),
            });
        }
        
        Ok(DownloadManifest {
            game_id: game_id.to_string(),
            game_name,
            game_path,
            chunks,
            overall_status,
            progress: DownloadProgress {
                total_chunks: total_chunks as usize,
                downloaded_chunks: downloaded_chunks as usize,
                percentage,
            },
        })
    }
    
    /// Markiert einen Chunk als heruntergeladen (atomar)
    pub fn mark_chunk_downloaded(&self, game_id: &str, chunk_hash: &str) -> SqliteResult<()> {
        let mut conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        let tx = conn.transaction()?;
        
        // Finde file_id für diesen Chunk
        let file_id: Option<i64> = tx.query_row(
            "SELECT file_id FROM chunks WHERE chunk_hash = ?1 AND file_id IN (SELECT id FROM files WHERE game_id = ?2)",
            params![chunk_hash, game_id],
            |row| row.get(0)
        ).ok();
        
        if let Some(file_id) = file_id {
            // Update chunk
            tx.execute(
                "UPDATE chunks SET is_downloaded = 1, downloaded_at = ?1 WHERE chunk_hash = ?2 AND file_id = ?3",
                params![now, chunk_hash, file_id],
            )?;
            
            // Update file status
            let downloaded_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_id = ?1 AND is_downloaded = 1",
                params![file_id],
                |row| row.get(0)
            )?;
            
            let total_count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_id = ?1",
                params![file_id],
                |row| row.get(0)
            )?;
            
            if downloaded_count == total_count {
                // Alle Chunks sind heruntergeladen - hole file_path für spätere Validierung
                // (Validierung wird nach dem Commit durchgeführt, um Fehler zu vermeiden)
                let file_path: Option<String> = tx.query_row(
                    "SELECT file_path FROM files WHERE id = ?1",
                    params![file_id],
                    |row| row.get(0)
                ).ok();
                
                // Setze Status zunächst auf Complete (wird später validiert)
                tx.execute(
                    "UPDATE files SET status = 'Complete' WHERE id = ?1",
                    params![file_id],
                )?;
                
                // Speichere file_path für spätere Validierung (außerhalb der Transaktion)
                // TODO: Validierung nach dem Commit durchführen
                if let Some(_file_path) = file_path {
                    // Validierung wird später in update_download_progress durchgeführt
                }
            } else {
                tx.execute(
                    "UPDATE files SET status = 'Downloading' WHERE id = ?1",
                    params![file_id],
                )?;
            }
            
            // Update download progress
            let total_downloaded: i64 = tx.query_row(
                "SELECT COUNT(*) FROM chunks WHERE is_downloaded = 1 AND file_id IN (SELECT id FROM files WHERE game_id = ?1)",
                params![game_id],
                |row| row.get(0)
            )?;
            
            let total_chunks: i64 = tx.query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_id IN (SELECT id FROM files WHERE game_id = ?1)",
                params![game_id],
                |row| row.get(0)
            )?;
            
            let percentage = if total_chunks > 0 {
                (total_downloaded as f64 / total_chunks as f64) * 100.0
            } else {
                0.0
            };
            
            tx.execute(
                "UPDATE downloads SET downloaded_chunks = ?1, percentage = ?2, updated_at = ?3 WHERE game_id = ?4",
                params![total_downloaded, percentage, now, game_id],
            )?;
            
            // Prüfe ob alle Dateien komplett sind
            let all_complete: bool = tx.query_row(
                "SELECT (SELECT COUNT(*) FROM files WHERE game_id = ?1 AND status = 'Complete') = (SELECT COUNT(*) FROM files WHERE game_id = ?1)",
                params![game_id],
                |row| row.get(0)
            ).unwrap_or(false);
            
            if all_complete {
                tx.execute(
                    "UPDATE downloads SET overall_status = 'Complete' WHERE game_id = ?1",
                    params![game_id],
                )?;
            } else {
                tx.execute(
                    "UPDATE downloads SET overall_status = 'Downloading' WHERE game_id = ?1 AND overall_status != 'Paused'",
                    params![game_id],
                )?;
            }
        }
        
        tx.commit()?;
        
        // Validiere Dateien, die gerade als Complete markiert wurden (außerhalb der Transaktion)
        // Hole alle Dateien, die als Complete markiert wurden
        let conn = self.conn.lock().unwrap();
        let files_to_validate: Vec<(i64, String, String)> = conn.prepare(
            "SELECT f.id, f.file_path, d.game_path FROM files f 
             JOIN downloads d ON f.game_id = d.game_id 
             WHERE f.game_id = ?1 AND f.status = 'Complete'"
        )?
        .query_map(params![game_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
        
        drop(conn);
        
        // Validiere jede Datei
        for (file_id, file_path, _game_path) in files_to_validate {
            if let Ok(manifest_path) = crate::synch::get_manifest_path(game_id) {
                if let Ok(manifest) = crate::synch::DownloadManifest::load(&manifest_path) {
                    if let Err(e) = crate::synch::check_and_validate_single_file(game_id, &manifest, &file_path) {
                        eprintln!("⚠️ Validierung fehlgeschlagen für {}: {} - Status wird auf Downloading zurückgesetzt", file_path, e);
                        // Setze Status zurück auf Downloading
                        let conn = self.conn.lock().unwrap();
                        conn.execute(
                            "UPDATE files SET status = 'Downloading' WHERE id = ?1",
                            params![file_id],
                        )?;
                        // Setze auch overall_status zurück
                        conn.execute(
                            "UPDATE downloads SET overall_status = 'Downloading' WHERE game_id = ?1",
                            params![game_id],
                        )?;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Gibt alle fehlenden Chunks zurück
    pub fn get_missing_chunks(&self, game_id: &str) -> SqliteResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();

        // Optimiert: JOIN statt Subquery für bessere Performance
        let mut stmt = conn.prepare(
            "SELECT c.chunk_hash FROM chunks c
             INNER JOIN files f ON c.file_id = f.id
             WHERE f.game_id = ?1 AND c.is_downloaded = 0
             ORDER BY c.chunk_index"
        )?;

        let chunks_iter = stmt.query_map(params![game_id], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;

        let mut missing = Vec::new();
        for chunk_hash in chunks_iter {
            missing.push(chunk_hash?);
        }

        Ok(missing)
    }

    /// Gibt fehlende Chunks in Batches zurück (memory-effizient für große Manifeste)
    /// `limit`: Maximale Anzahl der zurückzugebenden Chunks
    /// `offset`: Offset für Pagination (0-basiert)
    pub fn get_missing_chunks_paginated(&self, game_id: &str, limit: usize, offset: usize) -> SqliteResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();

        // Optimiert: JOIN statt Subquery für bessere Performance
        let mut stmt = conn.prepare(
            "SELECT c.chunk_hash FROM chunks c
             INNER JOIN files f ON c.file_id = f.id
             WHERE f.game_id = ?1 AND c.is_downloaded = 0
             ORDER BY c.chunk_index
             LIMIT ?2 OFFSET ?3"
        )?;

        let chunks_iter = stmt.query_map(params![game_id, limit as i64, offset as i64], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;

        let mut missing = Vec::new();
        for chunk_hash in chunks_iter {
            missing.push(chunk_hash?);
        }

        Ok(missing)
    }

    /// Gibt die Gesamtanzahl fehlender Chunks zurück (ohne die Chunks selbst zu laden)
    pub fn count_missing_chunks(&self, game_id: &str) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();

        // Optimiert: JOIN statt Subquery für bessere Performance
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chunks c
             INNER JOIN files f ON c.file_id = f.id
             WHERE f.game_id = ?1 AND c.is_downloaded = 0",
            params![game_id],
            |row| row.get(0)
        )?;

        Ok(count as usize)
    }

    /// Iterator für fehlende Chunks (memory-effizient)
    /// Gibt Chunks in kleinen Batches zurück, ohne alles auf einmal zu laden
    pub fn stream_missing_chunks(&self, game_id: &str, batch_size: usize) -> impl Iterator<Item = SqliteResult<Vec<String>>> + '_ {
        let conn = self.conn.clone();
        let game_id = game_id.to_string();

        (0..).map(move |batch_index| {
            let offset = batch_index * batch_size;
            let conn = conn.lock().unwrap();

            // Optimiert: JOIN statt Subquery für bessere Performance
            let mut stmt = conn.prepare(
                "SELECT c.chunk_hash FROM chunks c
                 INNER JOIN files f ON c.file_id = f.id
                 WHERE f.game_id = ?1 AND c.is_downloaded = 0
                 ORDER BY c.chunk_index
                 LIMIT ?2 OFFSET ?3"
            )?;

            let chunks_iter = stmt.query_map(params![&game_id, batch_size as i64, offset as i64], |row| {
                Ok(row.get::<_, String>(0)?)
            })?;

            let mut batch = Vec::new();
            for chunk_hash in chunks_iter {
                batch.push(chunk_hash?);
            }

            // Wenn der Batch leer ist, sind wir fertig
            if batch.is_empty() {
                return Ok(vec![]);
            }

            Ok(batch)
        }).take_while(|result| {
            // Stoppe bei leerem Batch oder Fehler
            match result {
                Ok(batch) => !batch.is_empty(),
                Err(_) => false,
            }
        })
    }
    
    /// Gibt alle game_ids zurück
    pub fn list_game_ids(&self) -> SqliteResult<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        
        let mut stmt = conn.prepare("SELECT game_id FROM downloads")?;
        let ids_iter = stmt.query_map([], |row| {
            Ok(row.get::<_, String>(0)?)
        })?;
        
        let mut ids = Vec::new();
        for id in ids_iter {
            ids.push(id?);
        }
        
        Ok(ids)
    }
    
    /// Löscht einen Download
    pub fn delete_download(&self, game_id: &str) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM downloads WHERE game_id = ?1", params![game_id])?;
        Ok(())
    }
    
    /// Migriert von JSON zu SQLite
    pub fn migrate_from_json(&self, json_path: &Path) -> SqliteResult<()> {
        if !json_path.exists() {
            return Ok(()); // Keine Migration nötig
        }
        
        let content = std::fs::read_to_string(json_path)
            .map_err(|e| rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR_READ),
                Some(format!("Konnte JSON nicht lesen: {}", e))
            ))?;
        
        let manifest: DownloadManifest = serde_json::from_str(&content)
            .map_err(|e| rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
                Some(format!("Konnte JSON nicht parsen: {}", e))
            ))?;
        
        // Prüfe ob bereits in DB vorhanden (direkt, ohne list_game_ids)
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM downloads WHERE game_id = ?1)",
            params![manifest.game_id],
            |row| row.get(0),
        )?;
        
        if exists {
            return Ok(()); // Bereits migriert
        }
        
        self.create_download(&manifest)?;
        
        Ok(())
    }
}

/// Globale Manifest-DB-Instanz pro game_id
/// ManifestDB ist bereits thread-safe durch Arc<Mutex<Connection>>, daher kein zusätzliches Mutex nötig
static MANIFEST_DBS: std::sync::OnceLock<Arc<Mutex<HashMap<String, Arc<ManifestDB>>>>> = std::sync::OnceLock::new();

/// Entfernt eine Manifest-DB aus dem Cache (für Tests)
pub fn remove_manifest_db_from_cache(game_id: &str) {
    if let Some(dbs) = MANIFEST_DBS.get() {
        let mut dbs_map = dbs.lock().unwrap();
        dbs_map.remove(game_id);
    }
}

/// Öffnet oder erstellt eine Manifest-DB für ein Spiel
pub fn get_manifest_db(game_id: &str) -> SqliteResult<Arc<ManifestDB>> {
    let dbs = MANIFEST_DBS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
    
    let mut dbs_map = dbs.lock().unwrap();
    
    if let Some(db) = dbs_map.get(game_id) {
        return Ok(db.clone());
    }
    
    // Erstelle neue DB
    let base_dir = directories::ProjectDirs::from("com", "deckdrop", "deckdrop")
        .ok_or_else(|| rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some("Konnte Konfigurationsverzeichnis nicht bestimmen".to_string())
        ))?;
    let config_dir = base_dir.config_dir();
    let games_dir = config_dir.join("games").join(game_id);
    let db_path = games_dir.join("manifest.db");
    
    // Migriere von JSON falls vorhanden
    let json_path = games_dir.join("manifest.json");
    let db = ManifestDB::open(&db_path)?;
    
    if json_path.exists() {
        if let Err(e) = db.migrate_from_json(&json_path) {
            eprintln!("Warnung: Fehler bei Migration von JSON zu SQLite: {}", e);
        } else {
            // Backup der alten JSON-Datei
            let backup_path = json_path.with_extension("json.backup");
            let _ = std::fs::rename(&json_path, &backup_path);
        }
    }
    
    let db_arc = Arc::new(db);
    dbs_map.insert(game_id.to_string(), db_arc.clone());
    
    Ok(db_arc)
}

