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

/// Globale Set von requested_chunks (für Synchronisation mit Scheduler-Cache)
/// Wird vom UI-Thread aktualisiert und vom Scheduler-Thread gelesen.
static REQUESTED_CHUNKS_GLOBAL: std::sync::OnceLock<Arc<Mutex<std::collections::HashSet<String>>>> = std::sync::OnceLock::new();

/// Setzt die globale requested_chunks Set (wird vom UI-Thread aufgerufen)
pub fn set_requested_chunks_global(requested: Arc<Mutex<std::collections::HashSet<String>>>) {
    let _ = REQUESTED_CHUNKS_GLOBAL.set(requested);
}

/// Gibt die globale requested_chunks Set zurück
pub fn get_requested_chunks_global() -> Option<Arc<Mutex<std::collections::HashSet<String>>>> {
    REQUESTED_CHUNKS_GLOBAL.get().cloned()
}

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

                // WICHTIG: Cache-Cleanup ZUERST, bevor wir prüfen ob der Cache voll ist!
                // ROBUSTE LÖSUNG: Einfache und zuverlässige Cleanup-Logik
                // Wir entfernen Chunks, die:
                // 1. Zu alt sind (Timeout, älter als 30 Sekunden) - definitiv hängende Requests
                // 2. Nicht mehr in requested_chunks sind UND mindestens 2 Sekunden alt - fertig (mit kleinem Puffer für Race Conditions)
                // Der 2-Sekunden-Puffer ist ausreichend für Event-Verarbeitung, aber kurz genug, um fertige Chunks schnell zu entfernen
                if let Some(requested_global) = get_requested_chunks_global() {
                    if let Ok(requested) = requested_global.lock() {
                        // Entferne Chunks, die:
                        // - Zu alt sind (Timeout, > 30 Sekunden) ODER
                        // - Nicht mehr in requested_chunks sind UND mindestens 2 Sekunden alt (fertig, mit kleinem Puffer)
                        requested_chunks_cache.retain(|hash, time| {
                            let age = time.elapsed();
                            let is_timeout = age >= Duration::from_secs(30);
                            let still_requested = requested.contains(hash);
                            let is_old_enough = age >= Duration::from_secs(2);
                            
                            // Behalte Chunk nur wenn:
                            // - Nicht zu alt (Timeout) UND
                            // - (Noch angefordert ODER sehr neu < 2 Sekunden)
                            !is_timeout && (still_requested || !is_old_enough)
                        });
                    } else {
                        // Fallback: Nur Timeouts entfernen, wenn Lock fehlschlägt
                        requested_chunks_cache.retain(|_hash, time| {
                            time.elapsed() < Duration::from_secs(30)
                        });
                    }
                } else {
                    // Fallback: Nur Timeouts entfernen, wenn requested_chunks nicht verfügbar ist
                    requested_chunks_cache.retain(|_hash, time| {
                        time.elapsed() < Duration::from_secs(30)
                    });
                }

                // Lade aktive Downloads (mit eigener Cache-Schicht im Core)
                let active_downloads = load_active_downloads();

                // Lade aktuelle Konfiguration für max_concurrent_chunks
                let config = Config::load();
                let max_chunks_per_peer = config.max_concurrent_chunks.max(1);
                
                // Globales Limit für gleichzeitige Requests (Backpressure-Schutz)
                // Der Disk-Writer Puffer ist 100. Wir sollten niemals mehr als ~80 Requests gleichzeitig offen haben.
                // Sonst laufen wir Gefahr, dass der Puffer voll läuft und Chunks verworfen werden (Liveloop).
                const GLOBAL_MAX_REQUESTS: usize = 80;
                let current_active_requests = requested_chunks_cache.len();
                if current_active_requests >= GLOBAL_MAX_REQUESTS {
                    // Warte, bis Requests abgearbeitet sind (aber Cache wurde bereits aufgeräumt!)
                    continue;
                }
                let available_slots = GLOBAL_MAX_REQUESTS - current_active_requests;

                // Sammle alle aktuell fehlenden Chunks aller aktiven Downloads (memory-effizient)
                // WICHTIG: Kontinuierliches Nachladen - wenn wir unter ein Limit kommen, laden wir nach
                // Das stellt sicher, dass der Download nicht stoppt, wenn die initialen Kandidaten verarbeitet sind.
                const CANDIDATE_LIMIT: usize = 200; // Max Kandidaten die wir insgesamt haben wollen
                const REFILL_THRESHOLD: usize = 50; // Wenn wir unter 50 kommen, füllen wir auf
                const REFILL_AMOUNT: usize = 100; // Wie viele neue Kandidaten wir nachladen
                
                // Prüfe ob wir nachladen müssen (wenn Cache zu klein wird)
                let needs_refill = current_active_requests < REFILL_THRESHOLD;
                
                // Lade fehlende Chunks (kontinuierlich, wenn nötig)
                let mut all_currently_missing = HashSet::new();
                let mut candidates_count = 0;
                
                // Wenn wir nachladen müssen, laden wir mehr. Sonst nur so viele, wie wir brauchen.
                let target_limit = if needs_refill { CANDIDATE_LIMIT } else { current_active_requests.max(20) };
                
                'outer: for (game_id, manifest) in &active_downloads {
                    if matches!(manifest.overall_status, DownloadStatus::Downloading | DownloadStatus::Pending) {
                        // Verwende SQLite-Datenbank direkt für memory-effizienten Zugriff
                        if let Ok(db_arc) = deckdrop_core::manifest_db::get_manifest_db(game_id) {
                            for batch_result in db_arc.stream_missing_chunks(game_id, 100) {
                                if let Ok(batch) = batch_result {
                                    for chunk_hash in batch {
                                        // Überspringe Chunks, die bereits im Cache sind (wenn wir nicht nachfüllen)
                                        if !needs_refill && requested_chunks_cache.contains_key(&chunk_hash) {
                                            continue;
                                        }
                                        all_currently_missing.insert(chunk_hash);
                                        candidates_count += 1;
                                        if candidates_count >= target_limit {
                                            break 'outer;
                                        }
                                    }
                                } else {
                                    break; // Fehler beim Streamen
                                }
                            }
                        } else if let Ok(manifest_path) = deckdrop_core::synch::get_manifest_path(game_id) {
                            if let Ok(db) = deckdrop_core::manifest_db::ManifestDB::open(&manifest_path) {
                                for batch_result in db.stream_missing_chunks(game_id, 100) {
                                    if let Ok(batch) = batch_result {
                                        for chunk_hash in batch {
                                            if !needs_refill && requested_chunks_cache.contains_key(&chunk_hash) {
                                                continue;
                                            }
                                            all_currently_missing.insert(chunk_hash);
                                            candidates_count += 1;
                                            if candidates_count >= target_limit {
                                                break 'outer;
                                            }
                                        }
                                    } else { break; }
                                }
                            } else {
                                // Fallback: Verwende in-memory Manifest
                                let missing = manifest.get_missing_chunks();
                                for m in missing {
                                    if !needs_refill && requested_chunks_cache.contains_key(&m) {
                                        continue;
                                    }
                                    all_currently_missing.insert(m);
                                    candidates_count += 1;
                                    if candidates_count >= target_limit {
                                        break 'outer;
                                    }
                                }
                            }
                        }
                    }
                }
                
                // DEBUG: Scheduler Status
                if current_active_requests < 10 || needs_refill {
                   eprintln!("SCHEDULER: Missing={} Cached={} Slots={} Refill={}", 
                       all_currently_missing.len(), requested_chunks_cache.len(), available_slots, needs_refill);
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
                                // WICHTIG: Verwende get_manifest_db für Connection-Pooling
                                let missing_chunks = if let Ok(db_arc) = deckdrop_core::manifest_db::get_manifest_db(game_id) {
                                    let mut all_missing = Vec::new();
                                    for batch_result in db_arc.stream_missing_chunks(game_id, 50) {
                                        if let Ok(batch) = batch_result {
                                            all_missing.extend(batch);
                                        }
                                    }
                                    all_missing
                                } else if let Ok(manifest_path) = deckdrop_core::synch::get_manifest_path(game_id) {
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
                                
                                // Limit berechnen (Peer-Limit UND Globales Limit beachten)
                                let total_max_chunks = (max_chunks_per_peer * peers.len().max(1)).min(available_slots);
                                
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

