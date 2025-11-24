//! Network-Bridge: Verbindet Tokio Network-Thread mit Iced UI-Thread

use deckdrop_network::network::discovery::{start_discovery, DiscoveryEvent, DownloadRequest, GamesLoader, GameMetadataLoader, ChunkLoader, MetadataUpdate};
use deckdrop_network::network::games::NetworkGameInfo;
use deckdrop_core::{Config, GameInfo, check_game_config_exists, load_active_downloads, request_missing_chunks, DownloadStatus};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::sync::mpsc;
use std::thread;

/// Shared State für Network-Bridge
pub struct NetworkBridge {
    pub event_rx: mpsc::Receiver<DiscoveryEvent>,
    pub download_request_tx: tokio::sync::mpsc::UnboundedSender<DownloadRequest>,
    pub metadata_update_tx: tokio::sync::mpsc::UnboundedSender<MetadataUpdate>,
}

/// Globaler Network Event Receiver (für einfachen Zugriff)
static NETWORK_EVENT_RX: std::sync::OnceLock<Arc<std::sync::Mutex<mpsc::Receiver<DiscoveryEvent>>>> = std::sync::OnceLock::new();

/// Globaler Download Request Sender (für einfachen Zugriff)
static DOWNLOAD_REQUEST_TX: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<DownloadRequest>> = std::sync::OnceLock::new();

/// Globaler Metadata Update Sender (für einfachen Zugriff)
static METADATA_UPDATE_TX: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<MetadataUpdate>> = std::sync::OnceLock::new();

/// Globale Map: Spiel-ID -> Peers, die für das Herunterladen dieses Spiels verwendet werden sollen
/// Dieser Zustand wird vom UI (bei Download-Start/Resume) gesetzt und vom Scheduler-Thread gelesen.
static SCHEDULER_DOWNLOADS: std::sync::OnceLock<Arc<Mutex<HashMap<String, Vec<String>>>>> = std::sync::OnceLock::new();

/// Setzt den globalen Network Event Receiver
pub fn set_network_event_rx(rx: Arc<std::sync::Mutex<mpsc::Receiver<DiscoveryEvent>>>) {
    let _ = NETWORK_EVENT_RX.set(rx);
}

/// Gibt den globalen Network Event Receiver zurück
pub fn get_network_event_rx() -> Option<Arc<std::sync::Mutex<mpsc::Receiver<DiscoveryEvent>>>> {
    NETWORK_EVENT_RX.get().cloned()
}

/// Setzt den globalen Download Request Sender
pub fn set_download_request_tx(tx: tokio::sync::mpsc::UnboundedSender<DownloadRequest>) {
    let _ = DOWNLOAD_REQUEST_TX.set(tx);
}

/// Gibt den globalen Download Request Sender zurück
pub fn get_download_request_tx() -> Option<tokio::sync::mpsc::UnboundedSender<DownloadRequest>> {
    DOWNLOAD_REQUEST_TX.get().cloned()
}

/// Setzt den globalen Metadata Update Sender
pub fn set_metadata_update_tx(tx: tokio::sync::mpsc::UnboundedSender<MetadataUpdate>) {
    let _ = METADATA_UPDATE_TX.set(tx);
}

/// Gibt den globalen Metadata Update Sender zurück
pub fn get_metadata_update_tx() -> Option<tokio::sync::mpsc::UnboundedSender<MetadataUpdate>> {
    METADATA_UPDATE_TX.get().cloned()
}

/// Registriert oder aktualisiert die Peers für ein Spiel im Scheduler
/// Gibt true zurück, wenn sich die Peers geändert haben oder das Spiel neu registriert wurde
pub fn register_download_peers(game_id: &str, peer_ids: &[String]) -> bool {
    let map = SCHEDULER_DOWNLOADS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())));
    if let Ok(mut guard) = map.lock() {
        if let Some(existing_peers) = guard.get(game_id) {
            if existing_peers == peer_ids {
                return false; // Keine Änderung
            }
        }
        guard.insert(game_id.to_string(), peer_ids.to_vec());
        return true;
    }
    false
}

/// Entfernt ein Spiel aus dem Scheduler (z.B. bei Cancel)
pub fn unregister_download(game_id: &str) {
    if let Some(map) = SCHEDULER_DOWNLOADS.get() {
        if let Ok(mut guard) = map.lock() {
            guard.remove(game_id);
        }
    }
}

impl NetworkBridge {
    /// Startet den Network-Thread und gibt eine NetworkBridge zurück
    pub fn start() -> Self {
        let (event_tx, event_rx) = mpsc::channel::<DiscoveryEvent>(32);
        
        // Channel für Download-Requests (vom UI-Thread zum Network-Thread)
        let (download_request_tx, download_request_rx) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        let download_request_tx_clone = download_request_tx.clone();
        
        // Channel für Metadata-Updates (vom UI-Thread zum Network-Thread)
        let (metadata_update_tx, metadata_update_rx) = tokio::sync::mpsc::unbounded_channel::<MetadataUpdate>();
        let metadata_update_tx_clone = metadata_update_tx.clone();
        
        // Lade Konfiguration
        let config = Config::load();
        let max_concurrent_chunks = config.max_concurrent_chunks;
        let player_name = Some(config.player_name.clone());
        
        // Berechne games_count
        // Nur das Verzeichnis selbst prüfen, NICHT rekursiv Unterverzeichnisse für game_paths
        let games_count = {
            let mut count = 0u32;
            for game_path in &config.game_paths {
                if check_game_config_exists(game_path) {
                    count += 1;
                }
                // KEINE rekursive Suche in Unterverzeichnissen für game_paths
                // Nur download_path wird rekursiv durchsucht
            }
            Some(count)
        };
        
        // Lade Keypair
        let keypair = Config::load_keypair();
        
        // Erstelle Callbacks
        let games_loader: Option<GamesLoader> = {
            Some(Arc::new(move || {
                let config = Config::load();
                let mut network_games = Vec::new();
                
                // Nur das Verzeichnis selbst prüfen, NICHT rekursiv Unterverzeichnisse für game_paths
                for game_path in &config.game_paths {
                    if check_game_config_exists(game_path) {
                        if let Ok(game_info) = GameInfo::load_from_path(game_path) {
                            network_games.push(NetworkGameInfo {
                                game_id: game_info.game_id,
                                name: game_info.name,
                                version: game_info.version,
                                start_file: game_info.start_file,
                                start_args: game_info.start_args,
                                description: game_info.description,
                                creator_peer_id: game_info.creator_peer_id,
                            });
                        }
                    }
                    // KEINE rekursive Suche in Unterverzeichnissen für game_paths
                    // Nur download_path wird rekursiv durchsucht
                }
                network_games
            }))
        };
        
        let game_metadata_loader: Option<GameMetadataLoader> = {
            Some(Arc::new(move |game_id: &str| {
                let config = Config::load();
                // Nur das Verzeichnis selbst prüfen, NICHT rekursiv Unterverzeichnisse für game_paths
                for game_path in &config.game_paths {
                    if check_game_config_exists(game_path) {
                        if let Ok(game_info) = GameInfo::load_from_path(game_path) {
                            if game_info.game_id == game_id {
                                let toml_path = game_path.join("deckdrop.toml");
                                let deckdrop_toml = std::fs::read_to_string(&toml_path).ok()?;
                                let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                                let deckdrop_chunks_toml = std::fs::read_to_string(&chunks_toml_path).ok()?;
                                return Some((deckdrop_toml, deckdrop_chunks_toml));
                            }
                        }
                    }
                    // KEINE rekursive Suche in Unterverzeichnissen für game_paths
                    // Nur download_path wird rekursiv durchsucht
                }
                None
            }))
        };
        
        let chunk_loader: Option<ChunkLoader> = {
            Some(Arc::new(move |chunk_hash: &str| {
                let config = Config::load();
                let parts: Vec<&str> = chunk_hash.split(':').collect();
                if parts.len() != 2 {
                    return None;
                }
                let file_hash = parts[0];
                let chunk_index = parts[1].parse::<usize>().ok()?;
                
                for game_path in &config.game_paths {
                    if check_game_config_exists(game_path) {
                        let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                        if let Ok(chunks_toml_content) = std::fs::read_to_string(&chunks_toml_path) {
                            #[derive(serde::Deserialize)]
                            struct ChunksToml {
                                file: Vec<ChunkFileEntry>,
                            }
                            #[derive(serde::Deserialize)]
                            struct ChunkFileEntry {
                                path: String,
                                file_hash: String,
                                chunk_count: i64,
                                #[allow(dead_code)]
                                file_size: i64,
                            }
                            
                            if let Ok(chunks_toml) = toml::from_str::<ChunksToml>(&chunks_toml_content) {
                                for entry in chunks_toml.file {
                                    if entry.file_hash == file_hash && chunk_index < entry.chunk_count as usize {
                                        let full_path = game_path.join(&entry.path);
                                        if let Ok(mut file) = std::fs::File::open(&full_path) {
                                            use std::io::{Read, Seek, SeekFrom};
                                            const CHUNK_SIZE: usize = 1 * 1024 * 1024; // 1MB (muss mit Core übereinstimmen!)
                                            let offset = (chunk_index as u64) * (CHUNK_SIZE as u64);
                                            if file.seek(SeekFrom::Start(offset)).is_ok() {
                                                let mut buffer = vec![0u8; CHUNK_SIZE];
                                                if let Ok(bytes_read) = file.read(&mut buffer) {
                                                    if bytes_read > 0 {
                                                        buffer.truncate(bytes_read);
                                                        return Some(buffer);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                None
            }))
        };
        
        // Starte Tokio Runtime in separatem Thread
        thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Konnte Tokio Runtime nicht erstellen");
            
            rt.block_on(async move {
                // Starte Network Discovery
                let _handle = start_discovery(
                    event_tx,
                    player_name,
                    games_count,
                    keypair,
                    games_loader,
                    game_metadata_loader,
                    chunk_loader,
                    Some(download_request_rx),
                    Some(metadata_update_rx),
                    max_concurrent_chunks,
                ).await;
                
                // Halte Runtime am Leben
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            });
        });

        // Starte separaten Scheduler-Thread für Chunk-Requests (nicht im UI-Thread)
        let scheduler_download_request_tx = download_request_tx_clone.clone();
        thread::spawn(move || {
            use std::time::{Duration, Instant};
            use std::collections::HashSet;
            
            // Cache für bereits angeforderte Chunks (Chunk-Hash -> Zeitstempel)
            // Verhindert, dass wir Requests spammen, während der Download läuft
            let mut requested_chunks_cache: HashMap<String, Instant> = HashMap::new();
            
                loop {
                    std::thread::sleep(Duration::from_millis(300));

                    // Cleanup: Hier könnte später ein Cleanup für failed peers hinzugefügt werden
                    // (aber failed peers werden im Network-Layer verwaltet)

                // Lade aktive Downloads (mit eigener Cache-Schicht im Core)
                let active_downloads = load_active_downloads();

                // Lade aktuelle Konfiguration für max_concurrent_chunks
                let config = Config::load();
                let max_chunks_per_peer = config.max_concurrent_chunks.max(1);
                
                // Sammle alle aktuell fehlenden Chunks aller aktiven Downloads (memory-effizient)
                let mut all_currently_missing = HashSet::new();
                for (game_id, manifest) in &active_downloads {
                    if matches!(manifest.overall_status, DownloadStatus::Downloading | DownloadStatus::Pending) {
                        // Verwende SQLite-Datenbank direkt für memory-effizienten Zugriff
                        if let Ok(manifest_path) = deckdrop_core::synch::get_manifest_path(game_id) {
                            if let Ok(db) = deckdrop_core::manifest_db::ManifestDB::open(&manifest_path) {
                                // Lade Chunks in kleinen Batches (max 100 pro Batch)
                                for batch_result in db.stream_missing_chunks(game_id, 100) {
                                    if let Ok(batch) = batch_result {
                                        for chunk_hash in batch {
                                            all_currently_missing.insert(chunk_hash);
                                        }
                                    }
                                }
                            } else {
                                // Fallback: Verwende in-memory Manifest (für bestehende Downloads)
                                let missing = manifest.get_missing_chunks();
                                for m in missing {
                                    all_currently_missing.insert(m);
                                }
                            }
                        }
                    }
                }
                
                // WICHTIG: Aufräumen des Caches basierend auf DB-Status UND Timeout
                // 1. Entferne Chunks, die nicht mehr in der DB als fehlend gelistet sind (fertig heruntergeladen)
                // 2. Entferne Chunks, die länger als 30 Sekunden im Cache sind (Timeout für hängende Requests)
                let cache_size_before = requested_chunks_cache.len();
                requested_chunks_cache.retain(|hash, time| {
                    // Behalte Chunk nur wenn:
                    // - Er noch in all_currently_missing ist (noch nicht heruntergeladen)
                    // - UND noch nicht zu alt ist (weniger als 30 Sekunden im Cache)
                    let still_missing = all_currently_missing.contains(hash);
                    let not_timeout = time.elapsed() < Duration::from_secs(30);
                    still_missing && not_timeout
                });
                let cache_size_after = requested_chunks_cache.len();
                
                // Logge nur bei signifikanten Änderungen (weniger Spam)
                if cache_size_before != cache_size_after && (cache_size_before - cache_size_after) >= 5 {
                    eprintln!("🧹 Cache aufgeräumt: {} → {} Chunks ({} entfernt)", 
                        cache_size_before, cache_size_after, cache_size_before - cache_size_after);
                }

                if let Some(map) = SCHEDULER_DOWNLOADS.get() {
                    if let Ok(guard) = map.lock() {
                        for (game_id, peers) in guard.iter() {
                            if peers.is_empty() {
                                continue;
                            }

                            // Finde Manifest für dieses Spiel
                            if let Some((_, manifest)) = active_downloads.iter().find(|(gid, _)| gid == game_id) {
                                // Nur für aktive Downloads (Pending oder Downloading)
                                if !matches!(
                                    manifest.overall_status,
                                    DownloadStatus::Downloading | DownloadStatus::Pending
                                ) {
                                    continue;
                                }

                                // Eigene Scheduling-Logik mit direkter Datenbank-Abfrage (memory-effizient)
                                let missing_chunks = if let Ok(manifest_path) = deckdrop_core::synch::get_manifest_path(game_id) {
                                    if let Ok(db) = deckdrop_core::manifest_db::ManifestDB::open(&manifest_path) {
                                        // Sammle alle fehlenden Chunks aus der Datenbank
                                        let mut all_missing = Vec::new();
                                        for batch_result in db.stream_missing_chunks(game_id, 50) {
                                            if let Ok(batch) = batch_result {
                                                all_missing.extend(batch);
                                            }
                                        }
                                        all_missing
                                    } else {
                                        // Fallback: Verwende in-memory Manifest
                                        manifest.get_missing_chunks()
                                    }
                                } else {
                                    manifest.get_missing_chunks()
                                };

                                if missing_chunks.is_empty() {
                                    continue;
                                }

                                // Filter: Nur Chunks, die noch NICHT im Cache sind
                                let candidates: Vec<String> = missing_chunks.iter()
                                    .filter(|c| !requested_chunks_cache.contains_key(*c))
                                    .cloned()
                                    .collect();
                                
                                if candidates.is_empty() {
                                    continue;
                                }
                                
                                // Limit berechnen
                                let total_max_chunks = max_chunks_per_peer * peers.len().max(1);
                                
                                // Wähle die nächsten N Chunks
                                let chunks_to_request: Vec<String> = candidates.iter()
                                    .take(total_max_chunks)
                                    .cloned()
                                    .collect();
                                
                                // Markiere als angefordert
                                for chunk_hash in &chunks_to_request {
                                    requested_chunks_cache.insert(chunk_hash.clone(), Instant::now());
                                }
                                
                                if !chunks_to_request.is_empty() {
                                    // Round-Robin Verteilung auf Peers (kopiert/angepasst aus Core)
                                    let mut peer_chunk_counts: HashMap<String, usize> = HashMap::new();
                                    for peer_id in peers.iter() {
                                        peer_chunk_counts.insert(peer_id.clone(), 0);
                                    }
                                    
                                    let mut peer_index = 0;
                                    
                                    for chunk_hash in chunks_to_request {
                                        // Round-Robin Start
                                        let start_index = peer_index;
                                        let mut selected_peer = None;
                                        let mut min_count = usize::MAX;
                                        let mut attempts = 0;
                                        
                                        while attempts < peers.len() {
                                            let current_peer = &peers[peer_index % peers.len()];
                                            let count = peer_chunk_counts.get(current_peer).copied().unwrap_or(0);
                                            
                                            if count < max_chunks_per_peer && count < min_count {
                                                min_count = count;
                                                selected_peer = Some(current_peer.clone());
                                                if count == 0 || (min_count > 0 && count < min_count / 2) {
                                                    break;
                                                }
                                            }
                                            
                                            peer_index = (peer_index + 1) % peers.len();
                                            attempts += 1;
                                            
                                            if peer_index == start_index && attempts > 0 {
                                                break;
                                            }
                                        }
                                        
                                        if selected_peer.is_none() {
                                            selected_peer = Some(peers[peer_index % peers.len()].clone());
                                            peer_index = (peer_index + 1) % peers.len();
                                        }
                                        
                                        if let Some(peer_id) = selected_peer {
                                            *peer_chunk_counts.entry(peer_id.clone()).or_insert(0) += 1;
                                            
                                            if let Err(e) = scheduler_download_request_tx.send(
                                                DownloadRequest::RequestChunk {
                                                    peer_id: peer_id.clone(),
                                                    chunk_hash: chunk_hash.clone(),
                                                    game_id: game_id.to_string(),
                                                }
                                            ) {
                                                eprintln!("Fehler beim Senden von Chunk-Request für {}: {}", chunk_hash, e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        
        Self {
            event_rx,
            download_request_tx: download_request_tx_clone,
            metadata_update_tx: metadata_update_tx_clone,
        }
    }
}

