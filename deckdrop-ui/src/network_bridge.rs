//! Network-Bridge: Verbindet Tokio Network-Thread mit Iced UI-Thread

use deckdrop_network::network::discovery::{start_discovery, DiscoveryEvent, DownloadRequest, GamesLoader, GameMetadataLoader, ChunkLoader, MetadataUpdate};
use deckdrop_network::network::games::NetworkGameInfo;
use deckdrop_core::{Config, GameInfo, check_game_config_exists};
use std::sync::Arc;
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
                                            const CHUNK_SIZE: usize = 100 * 1024 * 1024;
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
        
        Self {
            event_rx,
            download_request_tx: download_request_tx_clone,
            metadata_update_tx: metadata_update_tx_clone,
        }
    }
}

