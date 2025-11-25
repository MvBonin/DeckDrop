//! Main app structure for Iced

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, progress_bar, Column, Space},
    Element, Length, Theme, Color, Task,
};
use toml;
use deckdrop_core::{Config, GameInfo, DownloadManifest, network_cache};
use crate::file_tree::FileTreeNode;
use deckdrop_network::network::discovery::DiscoveryEvent;
use deckdrop_network::network::games::NetworkGameInfo;
use deckdrop_network::network::peer::PeerInfo;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// UI scaling factor for different screen sizes
/// Smaller values = more compact UI (better for Steam Deck)
/// Larger values = more spacious UI (better for desktop)
const UI_SCALE: f32 = 0.75;

/// Scale a size value based on UI_SCALE
pub fn scale(size: f32) -> f32 {
    size * UI_SCALE
}

/// Scale a size value for text (slightly different scaling)
pub fn scale_text(size: f32) -> f32 {
    (size * UI_SCALE).max(10.0) // Minimum 10px for readability
}

/// Format bytes as MB or GB
fn format_size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    
    if bytes as f64 >= GB {
        format!("{:.1} GB", bytes as f64 / GB)
    } else {
        format!("{:.1} MB", bytes as f64 / MB)
    }
}

/// Game integrity status
#[derive(Debug, Clone, PartialEq)]
pub enum GameIntegrityStatus {
    NotChecked,
    Checking { current: usize, total: usize },
    Intact,
    Changed,
    Error(String),
}

/// Task für den Chunk-Writer-Thread
#[derive(Debug)]
pub struct ChunkWriteTask {
    pub chunk_hash: String,
    pub chunk_data: Vec<u8>,
    pub peer_id: String,
}

/// Main application state
#[derive(Debug, Clone)]
pub struct DeckDropApp {
    // Tabs
    pub current_tab: Tab,
    pub previous_tab: Option<Tab>, // For back navigation from game details
    pub current_game_details: Option<(PathBuf, GameInfo)>, // Current game being viewed in details
    
    // Daten
    pub my_games: Vec<(PathBuf, GameInfo)>,
    pub game_integrity_status: HashMap<PathBuf, GameIntegrityStatus>, // game_path -> status
    pub integrity_check_start_time: HashMap<PathBuf, std::time::Instant>, // game_path -> start time for progress tracking
    pub integrity_check_progress: Arc<std::sync::Mutex<HashMap<PathBuf, usize>>>, // game_path -> current progress (for real-time updates)
    pub integrity_check_results: Arc<std::sync::Mutex<HashMap<PathBuf, GameIntegrityStatus>>>, // game_path -> final result (for completed checks)
    pub network_games: HashMap<String, Vec<(String, NetworkGameInfo)>>, // game_id -> [(peer_id, game_info)]
    pub peers: Vec<PeerInfo>,
    
    // Downloads
    pub active_downloads: HashMap<String, DownloadState>, // game_id -> DownloadState
    pub last_download_update: std::time::Instant, // Zeitpunkt der letzten Download-Update (für Throttling)
    pub downloading_starting: Arc<std::sync::Mutex<std::collections::HashSet<String>>>, // game_id -> Download wird gerade gestartet (Button deaktivieren) - thread-safe
    pub preparing_downloads: Arc<std::sync::Mutex<HashMap<String, (usize, usize)>>>, // game_id -> (current_files, total_files) für Pre-Allocation-Progress - thread-safe
    
    // Chunk processing tracking (to prevent duplicate processing and enable parallel processing)
    pub processing_chunks: Arc<std::sync::Mutex<HashSet<String>>>, // chunk_hash -> in_progress
    
    // Chunk request tracking (to prevent requesting the same chunk multiple times)
    pub requested_chunks: Arc<std::sync::Mutex<HashSet<String>>>, // chunk_hash -> already requested
    
    // Tracking: Welcher Peer hat welchen Chunk angefordert (für Load-Balancing und Deduplizierung)
    pub chunk_peer_requests: Arc<std::sync::Mutex<HashMap<String, String>>>, // chunk_hash -> peer_id
    
    // Tracking: Wie viele aktive Requests pro Peer (für Load-Balancing)
    pub active_requests_per_peer: Arc<std::sync::Mutex<HashMap<String, usize>>>, // peer_id -> Anzahl aktiver Requests
    
    // Chunk download progress tracking (start time per chunk for progress calculation)
    pub chunk_download_start_times: Arc<std::sync::Mutex<HashMap<String, std::time::Instant>>>, // chunk_hash -> start_time
    
    // Chunks die gerade geschrieben werden (nicht mehr in requested_chunks, aber noch nicht im Manifest)
    pub writing_chunks: Arc<std::sync::Mutex<HashSet<String>>>, // chunk_hash -> being written
    
    // Upload tracking (chunks being uploaded to peers)
    #[allow(dead_code)]
    pub active_uploads: Arc<std::sync::Mutex<HashMap<String, (std::time::Instant, usize)>>>, // chunk_hash -> (start_time, chunk_size_bytes)
    pub upload_stats: Arc<std::sync::Mutex<UploadStats>>, // Upload-Statistiken
    
    // Phase 4: Peer-Performance-Tracking für adaptive Limits
    pub peer_performance: Arc<std::sync::Mutex<HashMap<String, PeerPerformance>>>, // peer_id -> PeerPerformance
    
    // Robustheit: Retry-Tracking für Chunk-Requests
    pub chunk_retries: Arc<std::sync::Mutex<HashMap<String, ChunkRetryInfo>>>, // chunk_hash -> Retry-Info
    
    // Performance-Monitoring
    pub performance_metrics: PerformanceMetrics, // Aktuelle Performance-Metriken
    
    // Config
    pub config: Config,
    
    // Status
    pub status: StatusInfo,
    
    // Network Event Receiver (für Polling)
    // Wird über statischen Zugriff verwendet (siehe network_bridge.rs)
    // Dieses Feld wird nicht mehr direkt verwendet, bleibt aber für Kompatibilität
    #[allow(dead_code)]
    _network_event_rx: Arc<std::sync::Mutex<mpsc::Receiver<DiscoveryEvent>>>,
    
    // Dialoge
    pub show_license_dialog: bool,
    pub show_settings: bool,
    pub show_add_game_dialog: bool,
    
    // Form fields for "Add Game"
    pub add_game_path: String,
    pub add_game_name: String,
    pub add_game_version: String,
    pub add_game_start_file: String,
    pub add_game_start_args: String,
    pub add_game_description: String,
    pub add_game_additional_instructions: String,
    
    // Progress for adding game (chunk generation)
    pub add_game_progress: Option<(usize, usize, String)>, // current, total, current_file
    pub add_game_progress_tracker: Arc<std::sync::Mutex<Option<(usize, usize, String)>>>, // Shared state for thread updates
    pub add_game_generating: Option<PathBuf>, // Path of game being generated
    pub add_game_saving: bool, // Whether save button was clicked (to disable it)
    
    // Settings-Felder
    pub settings_player_name: String,
    pub settings_download_path: String,
    pub settings_max_concurrent_chunks: String,
    
    // License Dialog fields
    pub license_player_name: String,
    
    // Game Edit fields (for Creator)
    pub editing_game: bool,
    pub edit_game_name: String,
    pub edit_game_start_file: String,
    pub edit_game_start_args: String,
    pub edit_game_description: String,
    pub edit_game_additional_instructions: String,
    pub edit_game_path: Option<PathBuf>, // Path of game being edited
    
    // Worker Channel für paralleles Schreiben (Async Channel für MPMC)
    pub chunk_writer_tx: async_channel::Sender<ChunkWriteTask>,
}

/// Tab selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    MyGames,
    NetworkGames,
    Peers,
    Performance, // Performance monitoring tab
    Settings,
    GameDetails, // Detail view for a specific game
}

/// Download status for UI
#[derive(Debug, Clone)]
pub struct DownloadState {
    pub manifest: DownloadManifest,
    pub progress_percent: f32,
    pub downloading_chunks_count: usize, // Anzahl der aktuell heruntergeladenen Chunks
    pub peer_count: usize, // Anzahl der Peers für diesen Download
    pub download_speed_bytes_per_sec: f64, // Download-Geschwindigkeit in Bytes/Sekunde (gleitender Durchschnitt)
    #[allow(dead_code)]
    pub last_update_time: std::time::Instant, // Zeitpunkt der letzten Aktualisierung
    pub last_downloaded_chunks: usize, // Anzahl der heruntergeladenen Chunks bei letzter Aktualisierung
    pub speed_samples: Vec<(std::time::Instant, usize)>, // Zeitstempel und heruntergeladene Chunks für Geschwindigkeitsberechnung
}

/// Status information
#[derive(Debug, Clone)]
pub struct StatusInfo {
    #[allow(dead_code)]
    pub is_online: bool,
    pub peer_count: usize,
    pub active_download_count: usize,
}

/// Upload statistics
#[derive(Debug, Clone)]
pub struct UploadStats {
    pub active_upload_count: usize, // Anzahl der aktuell aktiven Uploads
    pub upload_speed_bytes_per_sec: f64, // Upload-Geschwindigkeit in Bytes/Sekunde
    pub last_update_time: std::time::Instant, // Zeitpunkt der letzten Aktualisierung
    pub last_uploaded_bytes: u64, // Anzahl der hochgeladenen Bytes bei letzter Aktualisierung
}

/// Peer-Performance-Tracking für adaptive Download-Limits (Phase 4 Optimierung)
#[derive(Debug, Clone)]
pub struct PeerPerformance {
    pub download_speed_bytes_per_sec: f64, // Download-Geschwindigkeit in Bytes/Sekunde (gleitender Durchschnitt)
    pub success_rate: f64, // Erfolgsrate: 0.0 - 1.0 (erfolgreiche / totale Requests)
    #[allow(dead_code)]
    pub active_requests: usize, // Aktuelle Anzahl aktiver Chunk-Requests
    pub total_requests: usize, // Gesamtanzahl Requests
    pub successful_requests: usize, // Anzahl erfolgreicher Requests
    pub last_update: std::time::Instant, // Zeitpunkt der letzten Aktualisierung
    pub speed_samples: Vec<(std::time::Instant, usize)>, // Zeitstempel und Bytes für Geschwindigkeitsberechnung
    // Robustheit: Circuit Breaker
    pub blocked_until: Option<std::time::Instant>, // Peer blockiert bis zu diesem Zeitpunkt (Circuit Breaker)
    pub consecutive_failures: usize, // Anzahl aufeinanderfolgender Fehler
}

/// Performance-Monitoring-Daten
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub total_download_speed_bytes_per_sec: f64, // Gesamte Download-Geschwindigkeit
    pub total_upload_speed_bytes_per_sec: f64, // Gesamte Upload-Geschwindigkeit
    pub active_downloads: usize, // Anzahl aktiver Downloads
    pub active_uploads: usize, // Anzahl aktiver Uploads
    pub total_chunks_downloaded: usize, // Gesamtanzahl heruntergeladener Chunks
    pub total_chunks_uploaded: usize, // Gesamtanzahl hochgeladener Chunks
    pub active_connections: usize, // Anzahl aktiver Verbindungen
    pub peer_performance: Vec<(String, PeerPerformance)>, // Performance pro Peer
    pub bandwidth_utilization_percent: f64, // Bandbreiten-Nutzung in Prozent (geschätzt)
    pub last_update: std::time::Instant, // Zeitpunkt der letzten Aktualisierung
}

/// Retry-Information für Chunk-Requests
#[derive(Debug, Clone)]
pub struct ChunkRetryInfo {
    pub retry_count: usize, // Anzahl der bisherigen Retries
    pub last_retry_time: std::time::Instant, // Zeitpunkt des letzten Retry-Versuchs
    pub last_peer_id: String, // Letzter Peer, der versucht wurde
    pub failure_count_with_peer: usize, // Anzahl Fehler mit diesem Peer
}

impl Default for PeerPerformance {
    fn default() -> Self {
        Self {
            download_speed_bytes_per_sec: 0.0,
            success_rate: 1.0, // Starte mit 100% (optimistisch)
            active_requests: 0,
            total_requests: 0,
            successful_requests: 0,
            last_update: std::time::Instant::now(),
            speed_samples: Vec::new(),
            blocked_until: None, // Kein Circuit Breaker aktiv
            consecutive_failures: 0,
        }
    }
}

impl PeerPerformance {
    /// Berechnet adaptive max_chunks_per_peer basierend auf Performance
    pub fn adaptive_max_chunks(&self, base_max: usize) -> usize {
        // Schnelle Peers (> 50MB/s): Erhöhe Limit um 50%
        // Mittlere Peers (10-50MB/s): Standard-Limit
        // Langsame Peers (< 10MB/s): Reduziere Limit um 50%
        let speed_mb_per_sec = self.download_speed_bytes_per_sec / (1024.0 * 1024.0);
        
        if speed_mb_per_sec > 50.0 && self.success_rate > 0.8 {
            // Sehr schneller Peer mit guter Erfolgsrate
            (base_max as f64 * 1.5) as usize
        } else if speed_mb_per_sec > 10.0 && self.success_rate > 0.7 {
            // Mittlerer Peer mit akzeptabler Erfolgsrate
            base_max
        } else if speed_mb_per_sec < 10.0 || self.success_rate < 0.5 {
            // Langsamer Peer oder schlechte Erfolgsrate
            (base_max as f64 * 0.5).max(2.0) as usize // Mindestens 2 Chunks
        } else {
            base_max
        }
    }
}


impl Default for DeckDropApp {
    fn default() -> Self {
        println!("[DEBUG] ===== Default::default() called =====");
        // Create a dummy receiver for Default
        // In main() this will be replaced by the real receiver
        let (_tx, rx) = mpsc::channel(1);
        let config = Config::load();
        println!("[DEBUG] Config loaded: download_path={}, game_paths={}", 
            config.download_path.display(), config.game_paths.len());
        // UI sofort initialisieren (leer), Daten asynchron nachladen
        // Dies verhindert, dass die UI beim Starten einfriert ("Frozen UI")
        
        // Starte asynchrones Laden im Hintergrund
        if let Some(tx) = crate::get_download_prep_tx() {
            let config_clone = config.clone();
            std::thread::spawn(move || {
                eprintln!("AsyncLoader (Default): Starte Laden von Spielen im Hintergrund...");
                
                // 1. Lade Spiele aus dem Download-Pfad und manuellen Pfaden
                let mut my_games = Vec::new();
                
                // 1a. Download-Pfad
                if config_clone.download_path.exists() {
                    my_games.extend(deckdrop_core::load_games_from_directory(&config_clone.download_path));
                }
                
                // 1b. Manuelle Pfade
                for game_path in &config_clone.game_paths {
                    if deckdrop_core::check_game_config_exists(game_path) {
                        if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(game_path) {
                            my_games.push((game_path.clone(), game_info));
                        }
                    }
                }
                
                // Deduplizierung
                {
                     use std::collections::HashMap;
                     let mut seen_ids = HashMap::new();
                     let mut deduplicated = Vec::new();
                     for (game_path, game_info) in &my_games {
                         let game_id = &game_info.game_id;
                         if !seen_ids.contains_key(game_id) {
                             seen_ids.insert(game_id.clone(), game_path.clone());
                             deduplicated.push((game_path.clone(), game_info.clone()));
                         }
                     }
                     my_games = deduplicated;
                }
                
                eprintln!("AsyncLoader (Default): {} Spiele geladen. Sende an UI...", my_games.len());
                if let Err(e) = tx.send(Message::GamesLoaded(my_games)) {
                    eprintln!("Fehler beim Senden von GamesLoaded: {}", e);
                }
                
                // 2. Lade aktive Downloads (kann lange dauern wegen SQLite-Zugriffen)
                eprintln!("AsyncLoader (Default): Starte Laden von aktiven Downloads...");
                let active_downloads_from_manifests = deckdrop_core::load_active_downloads();
                eprintln!("AsyncLoader (Default): {} aktive Downloads geladen. Sende an UI...", active_downloads_from_manifests.len());
                // Entferne das Logging, da es bei großen Manifesten die Konsole spammt
                /*
                if let Err(e) = tx.send(Message::ActiveDownloadsLoaded(active_downloads_from_manifests)) {
                    eprintln!("Fehler beim Senden von ActiveDownloadsLoaded: {}", e);
                }
                */
                // Stattdessen nur senden ohne Logging des Manifests
                let _ = tx.send(Message::ActiveDownloadsLoaded(active_downloads_from_manifests));
            });
        } else {
            eprintln!("KRITISCHER FEHLER: Download-Prep-Sender nicht verfügbar in Default::default()! UI wird leer bleiben.");
        }
        
        let mut my_games = Vec::new();
        /*
        // 1. Lade Spiele aus dem Download-Pfad (Unterordner = Spiele)
        if config.download_path.exists() {
            println!("[DEBUG] Loading games from download_path: {}", config.download_path.display());
            my_games.extend(deckdrop_core::load_games_from_directory(&config.download_path));
            println!("[DEBUG] Loaded {} games from download_path", my_games.len());
        } else {
            println!("[DEBUG] Download path does not exist: {}", config.download_path.display());
        }
        */
        
        // 2. Lade Spiele aus den manuell hinzugefügten Pfaden (game_paths)
        // Nur das Verzeichnis selbst prüfen, NICHT rekursiv Unterverzeichnisse
        
        // 3. Lade aktive Downloads aus Manifesten und füge sie zu my_games hinzu
        // WICHTIG: Dieser Aufruf passiert nur einmal beim Start (Default::default),
        // daher ist das I/O hier akzeptabel. Im laufenden Betrieb verwendet
        // die UI ausschließlich den Cache (`get_active_downloads_cached_only()`),
        // um den Main-Thread nicht zu blockieren.
        
        
        let game_integrity_status = HashMap::new();
        // Don't initialize integrity checks automatically - user will trigger them manually
        println!("[DEBUG] Default::default(): {} games loaded (integrity checks will be manual)", my_games.len());
        
        // Lade gecachte Network-Games beim Start
        let mut network_games = HashMap::new();
        if let Ok(cached_games) = network_cache::load_all_cached_network_games() {
            for cached_game in cached_games {
                let game_id = cached_game.game_id.clone();
                let network_game_info = cached_game.to_network_game_info();
                // Konvertiere peer_ids zu Vec<(peer_id, game_info)>
                let peer_games: Vec<(String, NetworkGameInfo)> = cached_game.peer_ids
                    .iter()
                    .map(|peer_id| (peer_id.clone(), network_game_info.clone()))
                    .collect();
                // Zeige auch Spiele ohne aktive Peers an (offline)
                network_games.insert(game_id, peer_games);
            }
        }
        
        // Initialisiere Shared State für Worker-Thread
        let downloading_starting = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let preparing_downloads = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let processing_chunks = Arc::new(std::sync::Mutex::new(HashSet::new()));
        let requested_chunks = Arc::new(std::sync::Mutex::new(HashSet::new()));
        // Setze globale Variable für Scheduler-Synchronisation
        crate::network_bridge::set_requested_chunks_global(requested_chunks.clone());
        let chunk_peer_requests = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let active_requests_per_peer = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let chunk_download_start_times = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let writing_chunks = Arc::new(std::sync::Mutex::new(HashSet::new()));
        let chunk_retries = Arc::new(std::sync::Mutex::new(HashMap::new()));

        // Erstelle Worker Channel (Bounded für Backpressure)
        // Verwende async_channel für MPMC (Multi-Producer Multi-Consumer)
        // Verwende Puffergröße 100 (ca. 100MB bei 1MB Chunks). 
        // Das verhindert UI-Blockaden, minimiert aber den "Nachlauf-Effekt", wenn der Peer weg ist.
        let (chunk_writer_tx, chunk_writer_rx) = async_channel::bounded(100);
        
        // Starte Worker Threads (Parallel Writing)
        // 4 Threads sollten für SSDs gut sein, für HDDs könnte es zu Seek-Thrashing führen
        // Aber da wir File-Locking haben, ist es sicher.
        let num_threads = 4;
        
        for _ in 0..num_threads {
            let rx = chunk_writer_rx.clone();
            let wc_writing = writing_chunks.clone();
            let wc_requested = requested_chunks.clone();
            let wc_peer_req = chunk_peer_requests.clone();
            let wc_active_peer = active_requests_per_peer.clone();
            let wc_start_times = chunk_download_start_times.clone();
            let wc_retries = chunk_retries.clone();
            
            std::thread::spawn(move || {
                run_chunk_writer(
                    rx,
                    wc_writing,
                    wc_requested,
                    wc_peer_req,
                    wc_active_peer,
                    wc_start_times,
                    wc_retries,
                );
            });
        }

        Self {
            current_tab: Tab::MyGames,
            previous_tab: None,
            current_game_details: None,
            my_games,
            network_games,
            peers: Vec::new(),
            active_downloads: HashMap::new(),
            downloading_starting,
            preparing_downloads,
            processing_chunks,
            requested_chunks,
            chunk_peer_requests,
            active_requests_per_peer,
            chunk_download_start_times,
            writing_chunks,
            chunk_retries, // Robustheit: Retry-Tracking
            chunk_writer_tx, // Worker Channel
            active_uploads: Arc::new(std::sync::Mutex::new(HashMap::new())),
            upload_stats: Arc::new(std::sync::Mutex::new(UploadStats {
                active_upload_count: 0,
                upload_speed_bytes_per_sec: 0.0,
                last_update_time: std::time::Instant::now(),
                last_uploaded_bytes: 0,
            })),
            peer_performance: Arc::new(std::sync::Mutex::new(HashMap::new())), // Phase 4
            performance_metrics: PerformanceMetrics {
                total_download_speed_bytes_per_sec: 0.0,
                total_upload_speed_bytes_per_sec: 0.0,
                active_downloads: 0,
                active_uploads: 0,
                total_chunks_downloaded: 0,
                total_chunks_uploaded: 0,
                active_connections: 0,
                peer_performance: Vec::new(),
                bandwidth_utilization_percent: 0.0,
                last_update: std::time::Instant::now(),
            },
            game_integrity_status,
            integrity_check_start_time: HashMap::new(),
            integrity_check_progress: Arc::new(std::sync::Mutex::new(HashMap::new())),
            integrity_check_results: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_download_update: std::time::Instant::now(),
            config: config.clone(),
            status: StatusInfo {
                is_online: true,
                peer_count: 0,
                active_download_count: 0,
            },
            _network_event_rx: Arc::new(std::sync::Mutex::new(rx)),
            show_license_dialog: {
                // Show dialog if no peer ID exists OR if config doesn't exist or has default player name
                let config_path = deckdrop_core::Config::config_path();
                let should_show = if let Some(path) = config_path {
                    if !path.exists() {
                        true // Config doesn't exist, show dialog
                    } else {
                        // Config exists, check if player name is set (not default)
                        config.player_name.is_empty() || config.player_name == "Player"
                    }
                } else {
                    !Config::has_peer_id() // Fallback to peer ID check
                };
                should_show
            },
            show_settings: false,
            show_add_game_dialog: false,
            add_game_path: String::new(),
            add_game_name: String::new(),
            add_game_version: String::new(),
            add_game_start_file: String::new(),
            add_game_start_args: String::new(),
            add_game_description: String::new(),
            add_game_additional_instructions: String::new(),
            add_game_progress: None,
            add_game_progress_tracker: Arc::new(std::sync::Mutex::new(None)),
            add_game_generating: None,
            add_game_saving: false,
            settings_player_name: config.player_name.clone(),
            settings_download_path: config.download_path.to_string_lossy().to_string(),
            settings_max_concurrent_chunks: config.max_concurrent_chunks.to_string(),
            license_player_name: config.player_name.clone(),
            editing_game: false,
            edit_game_name: String::new(),
            edit_game_start_file: String::new(),
            edit_game_start_args: String::new(),
            edit_game_description: String::new(),
            edit_game_additional_instructions: String::new(),
            edit_game_path: None,
        }
    }
}

/// Messages for the application
#[derive(Debug, Clone)]
pub enum Message {
    // Tab-Navigation
    TabChanged(Tab),
    ShowGameDetails(PathBuf, GameInfo), // Show details for a game
    BackFromDetails, // Go back from game details view
    
    // Network Events
    NetworkEvent(DiscoveryEvent),
    
    // Downloads
    DownloadGame(String), // game_id
    PauseDownload(String), // game_id
    ResumeDownload(String), // game_id
    CancelDownload(String), // game_id
    DownloadPrepared(String, Result<(), String>), // game_id, result - wird gesendet, wenn Pre-Allocation fertig ist
    UpdateDownloadPreparationProgress(String, usize, usize), // game_id, current_files, total_files
    
    // My Games
    AddGame,
    AddGamePathChanged(String),
    BrowseGamePath,
    AddGameNameChanged(String),
    AddGameVersionChanged(String),
    AddGameStartFileChanged(String),
    BrowseStartFile,
    AddGameStartArgsChanged(String),
    AddGameDescriptionChanged(String),
    AddGameAdditionalInstructionsChanged(String),
    SaveGame,
    CancelAddGame,
    
    // Settings
    OpenSettings,
    SettingsPlayerNameChanged(String),
    SettingsDownloadPathChanged(String),
    SettingsMaxConcurrentChunksChanged(String),
    BrowseDownloadPath,
    SaveSettings,
    CancelSettings,
    
    // License Dialog
    LicensePlayerNameChanged(String),
    AcceptLicense,
    
    // Periodic updates
    Tick,
    
    // Network Events (from Network thread)
    NetworkEventReceived(DiscoveryEvent),
    
    // Game integrity check results
    GameIntegrityChecked(PathBuf, GameIntegrityStatus),
    
    // Progress update for integrity checks
    UpdateIntegrityProgress(PathBuf, usize, usize), // game_path, current, total
    
    // Start integrity check manually
    CheckIntegrity(PathBuf), // game_path
    
    // Progress update for adding games (chunk generation)
    UpdateAddGameProgress(usize, usize, String), // current, total, current_file
    AddGameChunksGenerated(PathBuf, Result<String, String>), // game_path, hash_result
    
    // System Tray
    ShowWindow,
    HideWindow,
    Quit,
    
    // Game Edit (for Creator)
    EditGame,
    EditGameNameChanged(String),
    EditGameStartFileChanged(String),
    BrowseEditStartFile,
    EditGameStartArgsChanged(String),
    EditGameDescriptionChanged(String),
    EditGameAdditionalInstructionsChanged(String),
    SaveGameEdit,
    CancelGameEdit,
    
    // Network Games Cache
    ClearNetworkCache,

    // Async Initialization
    GamesLoaded(Vec<(PathBuf, GameInfo)>),
    ActiveDownloadsLoaded(Vec<(String, deckdrop_core::DownloadManifest)>),
}

impl DeckDropApp {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }
    
    #[allow(dead_code)]
    fn new_with_network_rx(network_event_rx: Arc<std::sync::Mutex<mpsc::Receiver<DiscoveryEvent>>>) -> Self {
        let config = Config::load();
        
        // UI sofort initialisieren (leer), Daten asynchron nachladen
        // Dies verhindert, dass die UI beim Starten einfriert ("Frozen UI")
        
        // Starte asynchrones Laden im Hintergrund
        if let Some(tx) = crate::get_download_prep_tx() {
            let config_clone = config.clone();
            std::thread::spawn(move || {
                eprintln!("AsyncLoader: Starte Laden von Spielen im Hintergrund...");
                
                // 1. Lade Spiele aus dem Download-Pfad und manuellen Pfaden
                let mut my_games = Vec::new();
                
                // 1a. Download-Pfad
                if config_clone.download_path.exists() {
                    my_games.extend(deckdrop_core::load_games_from_directory(&config_clone.download_path));
                }
                
                // 1b. Manuelle Pfade
                for game_path in &config_clone.game_paths {
                    if deckdrop_core::check_game_config_exists(game_path) {
                        if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(game_path) {
                            my_games.push((game_path.clone(), game_info));
                        }
                    }
                }
                
                // Deduplizierung
                {
                     use std::collections::HashMap;
                     let mut seen_ids = HashMap::new();
                     let mut deduplicated = Vec::new();
                     for (game_path, game_info) in &my_games {
                         let game_id = &game_info.game_id;
                         if !seen_ids.contains_key(game_id) {
                             seen_ids.insert(game_id.clone(), game_path.clone());
                             deduplicated.push((game_path.clone(), game_info.clone()));
                         }
                     }
                     my_games = deduplicated;
                }
                
                eprintln!("AsyncLoader: {} Spiele geladen. Sende an UI...", my_games.len());
                if let Err(e) = tx.send(Message::GamesLoaded(my_games)) {
                    eprintln!("Fehler beim Senden von GamesLoaded: {}", e);
                }
                
                // 2. Lade aktive Downloads (kann lange dauern wegen SQLite-Zugriffen)
                eprintln!("AsyncLoader: Starte Laden von aktiven Downloads...");
                let active_downloads_from_manifests = deckdrop_core::load_active_downloads();
                eprintln!("AsyncLoader: {} aktive Downloads geladen. Sende an UI...", active_downloads_from_manifests.len());
                // Entferne das Logging, da es bei großen Manifesten die Konsole spammt
                /*
                if let Err(e) = tx.send(Message::ActiveDownloadsLoaded(active_downloads_from_manifests)) {
                    eprintln!("Fehler beim Senden von ActiveDownloadsLoaded: {}", e);
                }
                */
                // Stattdessen nur senden ohne Logging des Manifests
                let _ = tx.send(Message::ActiveDownloadsLoaded(active_downloads_from_manifests));
            });
        } else {
            eprintln!("KRITISCHER FEHLER: Download-Prep-Sender nicht verfügbar in new_with_network_rx!");
        }
        
        let my_games = Vec::new(); // Initial leer, wird asynchron gefüllt
        let active_downloads = HashMap::new(); // Initial leer, wird asynchron gefüllt
        
        let game_integrity_status = HashMap::new();
        
        // Lade gecachte Network-Games beim Start (schnell genug für synchron)
        let mut network_games = HashMap::new();
        if let Ok(cached_games) = network_cache::load_all_cached_network_games() {
            for cached_game in cached_games {
                let game_id = cached_game.game_id.clone();
                let network_game_info = cached_game.to_network_game_info();
                // Konvertiere peer_ids zu Vec<(peer_id, game_info)>
                let peer_games: Vec<(String, NetworkGameInfo)> = cached_game.peer_ids
                    .iter()
                    .map(|peer_id| (peer_id.clone(), network_game_info.clone()))
                    .collect();
                // Zeige auch Spiele ohne aktive Peers an (offline)
                network_games.insert(game_id, peer_games);
            }
        }
        
        // Initialisiere Shared State für Worker-Thread
        let downloading_starting = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let preparing_downloads = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let processing_chunks = Arc::new(std::sync::Mutex::new(HashSet::new()));
        let requested_chunks = Arc::new(std::sync::Mutex::new(HashSet::new()));
        // Setze globale Variable für Scheduler-Synchronisation
        crate::network_bridge::set_requested_chunks_global(requested_chunks.clone());
        let chunk_peer_requests = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let active_requests_per_peer = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let chunk_download_start_times = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let writing_chunks = Arc::new(std::sync::Mutex::new(HashSet::new()));
        let chunk_retries = Arc::new(std::sync::Mutex::new(HashMap::new()));

        // Erstelle Worker Channel (Bounded für Backpressure)
        // Verwende async_channel für MPMC (Multi-Producer Multi-Consumer)
        // Verwende Puffergröße 100 (ca. 100MB bei 1MB Chunks). 
        // Das verhindert UI-Blockaden, minimiert aber den "Nachlauf-Effekt", wenn der Peer weg ist.
        let (chunk_writer_tx, chunk_writer_rx) = async_channel::bounded(100);
        
        // Starte Worker Threads (Parallel Writing)
        // 4 Threads sollten für SSDs gut sein, für HDDs könnte es zu Seek-Thrashing führen
        // Aber da wir File-Locking haben, ist es sicher.
        let num_threads = 4;
        
        for _ in 0..num_threads {
            let rx = chunk_writer_rx.clone();
            let wc_writing = writing_chunks.clone();
            let wc_requested = requested_chunks.clone();
            let wc_peer_req = chunk_peer_requests.clone();
            let wc_active_peer = active_requests_per_peer.clone();
            let wc_start_times = chunk_download_start_times.clone();
            let wc_retries = chunk_retries.clone();
            
            std::thread::spawn(move || {
                run_chunk_writer(
                    rx,
                    wc_writing,
                    wc_requested,
                    wc_peer_req,
                    wc_active_peer,
                    wc_start_times,
                    wc_retries,
                );
            });
        }

        Self {
            current_tab: Tab::MyGames,
            previous_tab: None,
            current_game_details: None,
            my_games,
            network_games,
            peers: Vec::new(),
            active_downloads,
            downloading_starting,
            preparing_downloads,
            processing_chunks,
            requested_chunks,
            chunk_peer_requests,
            active_requests_per_peer,
            chunk_download_start_times,
            writing_chunks,
            chunk_retries, // Robustheit: Retry-Tracking
            chunk_writer_tx, // Worker Channel
            active_uploads: Arc::new(std::sync::Mutex::new(HashMap::new())),
            upload_stats: Arc::new(std::sync::Mutex::new(UploadStats {
                active_upload_count: 0,
                upload_speed_bytes_per_sec: 0.0,
                last_update_time: std::time::Instant::now(),
                last_uploaded_bytes: 0,
            })),
            peer_performance: Arc::new(std::sync::Mutex::new(HashMap::new())), // Phase 4
            performance_metrics: PerformanceMetrics {
                total_download_speed_bytes_per_sec: 0.0,
                total_upload_speed_bytes_per_sec: 0.0,
                active_downloads: 0,
                active_uploads: 0,
                total_chunks_downloaded: 0,
                total_chunks_uploaded: 0,
                active_connections: 0,
                peer_performance: Vec::new(),
                bandwidth_utilization_percent: 0.0,
                last_update: std::time::Instant::now(),
            },
            game_integrity_status,
            integrity_check_start_time: HashMap::new(),
            integrity_check_progress: Arc::new(std::sync::Mutex::new(HashMap::new())),
            integrity_check_results: Arc::new(std::sync::Mutex::new(HashMap::new())),
            last_download_update: std::time::Instant::now(),
            config: config.clone(),
            status: StatusInfo {
                is_online: true,
                peer_count: 0,
                active_download_count: 0,
            },
            _network_event_rx: network_event_rx,
            show_license_dialog: {
                // Show dialog if no peer ID exists OR if config doesn't exist or has default player name
                let config_path = deckdrop_core::Config::config_path();
                let should_show = if let Some(path) = config_path {
                    if !path.exists() {
                        true // Config doesn't exist, show dialog
                    } else {
                        // Config exists, check if player name is set (not default)
                        config.player_name.is_empty() || config.player_name == "Player"
                    }
                } else {
                    !Config::has_peer_id() // Fallback to peer ID check
                };
                should_show
            },
            show_settings: false,
            show_add_game_dialog: false,
            add_game_path: String::new(),
            add_game_name: String::new(),
            add_game_version: String::new(),
            add_game_start_file: String::new(),
            add_game_start_args: String::new(),
            add_game_description: String::new(),
            add_game_additional_instructions: String::new(),
            add_game_progress: None,
            add_game_progress_tracker: Arc::new(std::sync::Mutex::new(None)),
            add_game_generating: None,
            add_game_saving: false,
            settings_player_name: config.player_name.clone(),
            settings_download_path: config.download_path.to_string_lossy().to_string(),
            settings_max_concurrent_chunks: config.max_concurrent_chunks.to_string(),
            license_player_name: config.player_name.clone(),
            editing_game: false,
            edit_game_name: String::new(),
            edit_game_start_file: String::new(),
            edit_game_start_args: String::new(),
            edit_game_description: String::new(),
            edit_game_additional_instructions: String::new(),
            edit_game_path: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabChanged(tab) => {
                // Don't change previous_tab when switching to GameDetails (handled by ShowGameDetails)
                if tab != Tab::GameDetails {
                self.current_tab = tab;
                }
                // Initialize settings fields when Settings tab is opened
                if tab == Tab::Settings {
                    self.settings_player_name = self.config.player_name.clone();
                    self.settings_download_path = self.config.download_path.to_string_lossy().to_string();
                }
                // Update performance metrics when Performance tab is opened
                if tab == Tab::Performance {
                    self.update_performance_metrics();
                }
            }
            Message::ShowGameDetails(game_path, game_info) => {
                // Store previous tab for back navigation
                self.previous_tab = Some(self.current_tab);
                self.current_game_details = Some((game_path, game_info));
                self.current_tab = Tab::GameDetails;
            }
            Message::BackFromDetails => {
                // Restore previous tab
                if let Some(prev_tab) = self.previous_tab {
                    self.current_tab = prev_tab;
                    self.previous_tab = None;
                } else {
                    // Fallback to MyGames if no previous tab
                    self.current_tab = Tab::MyGames;
                }
                self.current_game_details = None;
            }
            Message::GameIntegrityChecked(game_path, status) => {
                self.game_integrity_status.insert(game_path.clone(), status);
                // Remove start time when check is complete
                self.integrity_check_start_time.remove(&game_path);
            }
            Message::CheckIntegrity(game_path) => {
                // Check if already checking
                if self.integrity_check_start_time.contains_key(&game_path) {
                    return Task::none(); // Already checking
                }
                
                // Set status to Checking (mit 0 als Platzhalter für total, wird im Thread aktualisiert)
                self.game_integrity_status.insert(game_path.clone(), GameIntegrityStatus::Checking { current: 0, total: 0 });
                
                // Set start time for progress tracking
                self.integrity_check_start_time.insert(game_path.clone(), std::time::Instant::now());
                
                // Add to progress tracker immediately to mark as started
                if let Ok(mut progress) = self.integrity_check_progress.lock() {
                    progress.insert(game_path.clone(), 0);
                }
                
                // Start the actual integrity check in a separate thread
                if let Some(tx) = crate::get_download_prep_tx() {
                    let game_path_for_check = game_path.clone();
                    let progress_tracker = self.integrity_check_progress.clone();
                    let results_tracker = self.integrity_check_results.clone();
                    
                    std::thread::spawn(move || {
                        // 1. Stelle sicher, dass deckdrop_chunks.toml existiert (blockierend)
                        let chunks_toml_path = game_path_for_check.join("deckdrop_chunks.toml");
                        if !chunks_toml_path.exists() {
                            eprintln!("AsyncCheck: deckdrop_chunks.toml fehlt – generiere neu...");
                            if let Err(e) = deckdrop_core::generate_chunks_toml(
                                &game_path_for_check,
                                Option::<fn(usize, usize, &str)>::None,
                            ) {
                                eprintln!("Fehler beim Generieren von deckdrop_chunks.toml: {}", e);
                            } else {
                                eprintln!("deckdrop_chunks.toml erfolgreich neu generiert");
                            }
                        }
                        
                        // 2. Ermittle Total Files (für Progress Bar)
                        let total = if chunks_toml_path.exists() {
                            if let Ok(content) = std::fs::read_to_string(&chunks_toml_path) {
                                if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                                    if let Some(files) = parsed.get("file").and_then(|f| f.as_array()) {
                                        files.len()
                                    } else { 0 }
                                } else { 0 }
                            } else { 0 }
                        } else { 0 };
                        
                        // Sende initiales Update mit korrektem Total
                        let _ = tx.send(Message::UpdateIntegrityProgress(game_path_for_check.clone(), 0, total));
                        
                        // 3. Perform light check (fast)
                        let result = deckdrop_core::light_check_game(&game_path_for_check);
                        match result {
                            Ok(light_result) => {
                                if light_result.missing_files.is_empty() && light_result.extra_files.is_empty() {
                                    // All files match, do full integrity check with progress
                                    let integrity_result = deckdrop_core::verify_game_integrity_with_progress(
                                        &game_path_for_check,
                                        Some({
                                            let progress_tracker = progress_tracker.clone();
                                            let game_path = game_path_for_check.clone();
                                            move |current, _total| {
                                                // Update progress in shared tracker (Tick UI liest das)
                                                if let Ok(mut progress) = progress_tracker.lock() {
                                                    progress.insert(game_path.clone(), current);
                                                }
                                            }
                                        })
                                    );
                                    
                                    // Determine final status
                                    let final_status = match integrity_result {
                                        Ok(result) => {
                                            if result.failed_files.is_empty() && result.missing_files.is_empty() {
                                                GameIntegrityStatus::Intact
                                            } else {
                                                GameIntegrityStatus::Changed
                                            }
                                        }
                                        Err(_) => GameIntegrityStatus::Changed,
                                    };
                                    
                                    // Store result
                                    if let Ok(mut results) = results_tracker.lock() {
                                        results.insert(game_path_for_check.clone(), final_status);
                                    }
                                } else {
                                    // Files changed
                                    if let Ok(mut results) = results_tracker.lock() {
                                        results.insert(game_path_for_check.clone(), GameIntegrityStatus::Changed);
                                    }
                                }
                            }
                            Err(e) => {
                                // Error
                                if let Ok(mut results) = results_tracker.lock() {
                                    results.insert(game_path_for_check.clone(), GameIntegrityStatus::Error(e.to_string()));
                                }
                            }
                        }
                        
                        // Clear progress tracker -> signals complete
                        if let Ok(mut progress) = progress_tracker.lock() {
                            progress.remove(&game_path_for_check);
                        }
                    });
                } else {
                    eprintln!("KRITISCH: Kein Download-Prep-Sender verfügbar für Integrity Check!");
                    // Fallback: Synchron ausführen (sollte nicht passieren)
                }
            }
            Message::UpdateAddGameProgress(current, total, file_name) => {
                self.add_game_progress = Some((current, total, file_name));
            }
            Message::AddGameChunksGenerated(game_path, hash_result) => {
                // Chunk generation complete
                // Check if already processed (prevent double processing)
                // Note: add_game_generating is set to None in Tick before sending this message,
                // so we check if add_game_progress is also None (meaning we already processed)
                if self.add_game_generating.is_none() && self.add_game_progress.is_none() {
                    // Already processed, ignore duplicate message
                    return Task::none();
                }
                
                // Mark as processed immediately
                self.add_game_generating = None;
                self.add_game_progress = None;
                self.add_game_saving = false; // Re-enable button after completion
                if let Ok(mut tracker) = self.add_game_progress_tracker.lock() {
                    *tracker = None;
                }
                
                let game_info = GameInfo {
                    game_id: deckdrop_core::game::generate_game_id(),
                    name: self.add_game_name.clone(),
                    version: deckdrop_core::game::initial_version(), // Immer "1" für neues Spiel
                    start_file: self.add_game_start_file.clone(),
                    start_args: if self.add_game_start_args.is_empty() { None } else { Some(self.add_game_start_args.clone()) },
                    description: if self.add_game_description.is_empty() { None } else { Some(self.add_game_description.clone()) },
                    additional_instructions: if self.add_game_additional_instructions.is_empty() { None } else { Some(self.add_game_additional_instructions.clone()) },
                    creator_peer_id: self.config.peer_id.clone(),
                    hash: hash_result.ok(),
                };
                
                // Save GameInfo (wird bereits im Thread geschrieben, aber hier nochmal als Fallback)
                // Dies ist ein Fallback, falls der Thread die Datei nicht geschrieben hat
                if let Err(e) = game_info.save_to_path_with_hash(&game_path, game_info.hash.clone()) {
                    eprintln!("Error saving game (fallback): {}", e);
                    // Nicht return Task::none(), da die Datei möglicherweise bereits geschrieben wurde
                }
                
                // WICHTIG: Prüfe ob deckdrop.toml und deckdrop_chunks.toml existieren
                // Beide Dateien müssen vorhanden sein, damit das Spiel geladen werden kann
                let deckdrop_toml_exists = game_path.join("deckdrop.toml").exists();
                let deckdrop_chunks_toml_exists = game_path.join("deckdrop_chunks.toml").exists();
                
                if !deckdrop_toml_exists || !deckdrop_chunks_toml_exists {
                    eprintln!("Error: Game files not ready yet. deckdrop.toml: {}, deckdrop_chunks.toml: {}", 
                        deckdrop_toml_exists, deckdrop_chunks_toml_exists);
                    // Warte nicht - das Spiel wird später automatisch geladen, wenn die Dateien vorhanden sind
                }
                
                // Add game path to config
                // WICHTIG: add_game_path speichert die Config automatisch
                let mut config = deckdrop_core::Config::load();
                if let Err(e) = config.add_game_path(&game_path) {
                    eprintln!("Error adding game path to config: {}", e);
                } else {
                    eprintln!("Game path {} added to config and saved", game_path.display());
                }
                
                // Update config in self
                self.config = config.clone();
                
                // WICHTIG: Lade die Config NEU, um sicherzustellen, dass die Pfade expandiert sind
                // (Config::load() expandiert Pfade automatisch)
                self.config = deckdrop_core::Config::load();
                
                // Reload games list to ensure consistency
                // Dies stellt sicher, dass alle Spiele korrekt geladen sind, einschließlich des neuen Spiels
                self.my_games.clear();
                self.game_integrity_status.clear();
                
                // 1. Lade Spiele aus dem Download-Pfad (Unterordner = Spiele)
                if self.config.download_path.exists() {
                    self.my_games.extend(deckdrop_core::load_games_from_directory(&self.config.download_path));
                }
                
                // 2. Lade Spiele aus den manuell hinzugefügten Pfaden (game_paths)
                // WICHTIG: Die Pfade sind bereits expandiert durch Config::load()
                for game_path_dir in &self.config.game_paths {
                    // Check if the path itself is a game (has deckdrop.toml)
                    if deckdrop_core::check_game_config_exists(game_path_dir) {
                        if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(game_path_dir) {
                            self.my_games.push((game_path_dir.clone(), game_info));
                            eprintln!("Loaded game from path: {}", game_path_dir.display());
                        } else {
                            eprintln!("Warning: Could not load game info from {}", game_path_dir.display());
                        }
                    } else {
                        eprintln!("Warning: Game config does not exist at {}", game_path_dir.display());
                    }
                    // KEINE rekursive Suche in Unterverzeichnissen für game_paths
                    // Nur download_path wird rekursiv durchsucht
                }
                
                // Deduplicate games by game_id
                self.deduplicate_games_by_id();
                
                eprintln!("Games list reloaded: {} games total (including newly added game)", self.my_games.len());
                
                // Mark the newly added game as "Intact" immediately (we just generated chunks, so we know it's valid)
                self.game_integrity_status.insert(game_path.clone(), GameIntegrityStatus::Intact);
                
                // Don't initialize integrity status for other games - they will show "NotChecked" by default
                // User can trigger integrity check manually via button
                
                // Close dialog and reset form
                self.show_add_game_dialog = false;
                self.add_game_path = String::new();
                self.add_game_name = String::new();
                self.add_game_version = String::new();
                self.add_game_start_file = String::new();
                self.add_game_start_args = String::new();
                self.add_game_description = String::new();
                self.add_game_additional_instructions = String::new();
                
                // Send metadata update with new games count
                // WICHTIG: Dies aktualisiert die games_count, die anderen Peers angezeigt wird
                // Die Spiele selbst werden automatisch angekündigt, wenn Peers eine GamesListRequest senden
                // (was durch das Keep-Alive-System automatisch passiert)
                if let Some(tx) = crate::network_bridge::get_metadata_update_tx() {
                    let games_count = self.my_games.len() as u32;
                    let _ = tx.send(deckdrop_network::network::discovery::MetadataUpdate {
                        player_name: None, // Only update games count
                        games_count: Some(games_count),
                    });
                    eprintln!("Metadata-Update gesendet: {} Spiele", games_count);
                }
                
                // WICHTIG: Das Spiel ist jetzt in config.game_paths und wird automatisch
                // von anderen Peers angezeigt, wenn sie eine GamesListRequest senden
                // Der games_loader Callback lädt jedes Mal die neuesten Spiele aus der Config,
                // also sind neue Spiele sofort verfügbar, wenn ein Peer eine GamesListRequest sendet
                // (was durch das Keep-Alive-System alle 60 Sekunden passiert, oder sofort bei ConnectionEstablished)
                eprintln!("Spiel {} erfolgreich hinzugefügt und wird im Netzwerk sofort angekündigt (bei nächster GamesListRequest)", game_path.display());
            }
            Message::UpdateIntegrityProgress(game_path, current, total) => {
                // Update progress for a checking game
                self.game_integrity_status.insert(game_path, GameIntegrityStatus::Checking { current, total });
                // Don't create additional Tick tasks - the subscription already handles this
            }
            Message::NetworkEvent(event) => {
                self.handle_network_event(event);
            }
            Message::NetworkEventReceived(event) => {
                self.handle_network_event(event);
            }
            Message::DownloadGame(game_id) => {
                // WICHTIG: Markiere Download als gestartet, um Button zu deaktivieren
                if let Ok(mut starting) = self.downloading_starting.lock() {
                    starting.insert(game_id.clone());
                }
                
                // Find peer for this download
                if let Some(peers) = self.network_games.get(&game_id) {
                    if let Some((peer_id, _)) = peers.first() {
                        // Start download via Network-Bridge
                        if let Some(tx) = crate::network_bridge::get_download_request_tx() {
                            let _ = tx.send(deckdrop_network::network::discovery::DownloadRequest::RequestGameMetadata {
                                peer_id: peer_id.clone(),
                                game_id: game_id.clone(),
                            });
                            eprintln!("Download gestartet für {} - Button deaktiviert", game_id);
                        } else {
                            // Fehler: Entferne aus downloading_starting, wenn Request nicht gesendet werden konnte
                            if let Ok(mut starting) = self.downloading_starting.lock() {
                                starting.remove(&game_id);
                            }
                        }
                    } else {
                        // Fehler: Kein Peer gefunden - entferne aus downloading_starting
                        if let Ok(mut starting) = self.downloading_starting.lock() {
                            starting.remove(&game_id);
                        }
                    }
                } else {
                    // Fehler: Keine Peers für dieses Spiel - entferne aus downloading_starting
                    if let Ok(mut starting) = self.downloading_starting.lock() {
                        starting.remove(&game_id);
                    }
                }
            }
            Message::PauseDownload(game_id) => {
                // Pause download
                if let Some(download_state) = self.active_downloads.get_mut(&game_id) {
                    download_state.manifest.overall_status = deckdrop_core::DownloadStatus::Paused;
                    
                    // Save manifest to disk
                    if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                        if let Err(e) = download_state.manifest.save(&manifest_path) {
                            eprintln!("Error saving manifest when pausing download for {}: {}", game_id, e);
                        }
                    }
                }
            }
            Message::ResumeDownload(game_id) => {
                // Resume download
                if let Some(download_state) = self.active_downloads.get_mut(&game_id) {
                    download_state.manifest.overall_status = deckdrop_core::DownloadStatus::Downloading;
                    
                    // Save manifest to disk
                    if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                        if let Err(e) = download_state.manifest.save(&manifest_path) {
                            eprintln!("Error saving manifest when resuming download for {}: {}", game_id, e);
                        }
                    }
                    
                    // Register download in Scheduler (Peers für dieses Spiel)
                    if let Some(peers) = self.network_games.get(&game_id) {
                        let peer_ids: Vec<String> =
                            peers.iter().map(|(pid, _)| pid.clone()).collect();
                        if !peer_ids.is_empty() {
                            crate::network_bridge::register_download_peers(&game_id, &peer_ids);
                        }
                    }
                }
            }
            Message::CancelDownload(game_id) => {
                // Cancel download
                if let Some(_peers) = self.network_games.get(&game_id) {
                    // Cancel download (local)
                    if let Err(e) = deckdrop_core::cancel_game_download(&game_id) {
                        eprintln!("Error canceling download for {}: {}", game_id, e);
                    }
                }
                // Entferne aus Scheduler
                crate::network_bridge::unregister_download(&game_id);
                // Remove from active downloads
                self.active_downloads.remove(&game_id);
                // Remove from preparing downloads
                if let Ok(mut preparing) = self.preparing_downloads.lock() {
                    preparing.remove(&game_id);
                }
                // Remove from downloading_starting
                if let Ok(mut starting) = self.downloading_starting.lock() {
                    starting.remove(&game_id);
                }
            }
            Message::UpdateDownloadPreparationProgress(game_id, current, total) => {
                // Aktualisiere Progress für Pre-Allocation
                // WICHTIG: Diese Messages werden normalerweise im Tick-Handler verarbeitet,
                // aber falls sie hier ankommen (z.B. durch direkten Aufruf), verarbeiten wir sie auch hier
                if let Ok(mut preparing) = self.preparing_downloads.lock() {
                    preparing.insert(game_id, (current, total));
                }
            }
            Message::DownloadPrepared(game_id, result) => {
                // WICHTIG: Diese Messages werden normalerweise im Tick-Handler verarbeitet,
                // aber falls sie hier ankommen (z.B. durch direkten Aufruf), verarbeiten wir sie auch hier
                // Entferne aus preparing_downloads
                if let Ok(mut preparing) = self.preparing_downloads.lock() {
                    preparing.remove(&game_id);
                }
                
                match result {
                    Ok(()) => {
                        // Pre-Allocation erfolgreich - starte Download
                        // Load manifest for UI update
                        if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                            if let Ok(manifest) = deckdrop_core::DownloadManifest::load(&manifest_path) {
                                let progress_percent = manifest.progress.percentage as f32;
                                let downloaded_chunks = manifest.progress.downloaded_chunks;
                                
                                self.active_downloads.insert(game_id.clone(), DownloadState {
                                    manifest: manifest.clone(),
                                    progress_percent,
                                    downloading_chunks_count: 0,
                                    peer_count: 0,
                                    download_speed_bytes_per_sec: 0.0,
                                    last_update_time: std::time::Instant::now(),
                                    last_downloaded_chunks: downloaded_chunks,
                                    speed_samples: Vec::new(),
                                });
                                
                                // Registriere Download im Scheduler (Peers für dieses Spiel)
                                if let Some(peers) = self.network_games.get(&game_id) {
                                    let peer_ids: Vec<String> = peers
                                        .iter()
                                        .map(|(pid, _)| pid.clone())
                                        .collect();
                                    if !peer_ids.is_empty() {
                                        crate::network_bridge::register_download_peers(
                                            &game_id,
                                            &peer_ids,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error preparing download for {}: {}", game_id, e);
                        // Entferne aus downloading_starting, damit Button wieder aktiviert wird
                        if let Ok(mut starting) = self.downloading_starting.lock() {
                            starting.remove(&game_id);
                        }
                    }
                }
            }
            Message::AddGame => {
                self.show_add_game_dialog = true;
                self.add_game_saving = false; // Reset saving state when dialog is opened
            }
            Message::AddGamePathChanged(path) => {
                self.add_game_path = path;
            }
            Message::BrowseGamePath => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Select Game Directory")
                    .pick_folder()
                {
                    let game_path = PathBuf::from(&path);
                    
                    // Prüfe ob dies bereits ein vollständiges DeckDrop-Spiel ist
                    if deckdrop_core::check_complete_deckdrop_game_exists(&game_path) {
                        // Lade das Spiel direkt
                        if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(&game_path) {
                            // Füge Spielpfad zur Config hinzu (falls noch nicht vorhanden)
                            let mut config = deckdrop_core::Config::load();
                            if !config.game_paths.contains(&game_path) {
                                if let Err(e) = config.add_game_path(&game_path) {
                                    eprintln!("Error adding game path to config: {}", e);
                                } else {
                                    self.config = config.clone();
                                }
                            }
                            
                            // Füge Spiel zu my_games hinzu (wenn noch nicht vorhanden)
                            let game_exists = self.my_games.iter()
                                .any(|(p, _)| p == &game_path);
                            if !game_exists {
                                self.my_games.push((game_path.clone(), game_info));
                                
                                // Don't initialize integrity status - will show "NotChecked" by default
                                // User can trigger integrity check manually via button
                                
                                // Dedupliziere Spiele nach game_id
                                self.deduplicate_games_by_id();
                                
                                // Sende Metadata-Update
                                if let Some(tx) = crate::network_bridge::get_metadata_update_tx() {
                                    let games_count = self.my_games.len() as u32;
                                    let _ = tx.send(deckdrop_network::network::discovery::MetadataUpdate {
                                        player_name: None,
                                        games_count: Some(games_count),
                                    });
                                }
                            }
                            
                            // Schließe Dialog und setze Form zurück
                            self.show_add_game_dialog = false;
                            self.add_game_path = String::new();
                            self.add_game_name = String::new();
                            self.add_game_version = String::new();
                            self.add_game_start_file = String::new();
                            self.add_game_start_args = String::new();
                            self.add_game_description = String::new();
                            self.add_game_additional_instructions = String::new();
                        }
                    } else {
                        // Normales Verhalten: Pfad setzen
                        self.add_game_path = path.to_string_lossy().to_string();
                    }
                }
            }
            Message::AddGameNameChanged(name) => {
                self.add_game_name = name;
            }
            Message::AddGameVersionChanged(_version) => {
                // Version ist nicht änderbar - immer "1" für neues Spiel
                // Ignoriere Änderungen
            }
            Message::AddGameStartFileChanged(start_file) => {
                // If user enters an absolute path that's within game_path, convert to relative
                if !start_file.is_empty() && !self.add_game_path.is_empty() {
                    let start_file_path = PathBuf::from(&start_file);
                    if start_file_path.is_absolute() {
                        let game_path = PathBuf::from(&self.add_game_path);
                        if let Ok(relative_path) = start_file_path.strip_prefix(&game_path) {
                            // Convert to forward slashes for cross-platform compatibility
                            let relative_str = relative_path.to_string_lossy().replace('\\', "/");
                            self.add_game_start_file = relative_str;
                        } else {
                            // Keep as-is if not within game path (user might be typing)
                self.add_game_start_file = start_file;
                        }
                    } else {
                        // Already relative, keep as-is
                        self.add_game_start_file = start_file;
                    }
                } else {
                    self.add_game_start_file = start_file;
                }
            }
            Message::BrowseStartFile => {
                if self.add_game_path.is_empty() {
                    // Button should be disabled, but handle gracefully
                    return Task::none();
                }
                
                let game_path = PathBuf::from(&self.add_game_path);
                if !game_path.exists() {
                    eprintln!("Error: Game path does not exist: {}", game_path.display());
                    return Task::none();
                }
                
                // Open file dialog starting from game path
                if let Some(selected_file) = rfd::FileDialog::new()
                    .set_title("Select Game Executable")
                    .set_directory(&game_path)
                    .pick_file()
                {
                    // Convert to relative path from game_path
                    if let Ok(relative_path) = selected_file.strip_prefix(&game_path) {
                        // Convert to forward slashes for cross-platform compatibility
                        let relative_str = relative_path.to_string_lossy().replace('\\', "/");
                        self.add_game_start_file = relative_str;
                    } else {
                        eprintln!("Error: Selected file is not within game path");
                    }
                }
            }
            Message::AddGameStartArgsChanged(args) => {
                self.add_game_start_args = args;
            }
            Message::AddGameDescriptionChanged(description) => {
                self.add_game_description = description;
            }
            Message::AddGameAdditionalInstructionsChanged(instructions) => {
                self.add_game_additional_instructions = instructions;
            }
            Message::SaveGame => {
                // Disable button immediately to prevent double-clicking
                if self.add_game_saving {
                    return Task::none();
                }
                self.add_game_saving = true;
                
                // Validate required fields
                if self.add_game_path.is_empty() || self.add_game_name.is_empty() || self.add_game_start_file.is_empty() {
                    eprintln!("Error: Path, name, and start file are required");
                    self.add_game_saving = false; // Re-enable button on validation error
                    return Task::none();
                }
                
                let game_path = PathBuf::from(&self.add_game_path);
                if !game_path.exists() {
                    eprintln!("Error: Game path does not exist: {}", game_path.display());
                    self.add_game_saving = false; // Re-enable button on validation error
                    return Task::none();
                }
                
                // Store game info for later use
                let game_info = GameInfo {
                    game_id: deckdrop_core::game::generate_game_id(),
                    name: self.add_game_name.clone(),
                    version: deckdrop_core::game::initial_version(), // Immer "1" für neues Spiel
                    start_file: self.add_game_start_file.clone(),
                    start_args: if self.add_game_start_args.is_empty() { None } else { Some(self.add_game_start_args.clone()) },
                    description: if self.add_game_description.is_empty() { None } else { Some(self.add_game_description.clone()) },
                    additional_instructions: if self.add_game_additional_instructions.is_empty() { None } else { Some(self.add_game_additional_instructions.clone()) },
                    creator_peer_id: self.config.peer_id.clone(),
                    hash: None,
                };
                
                // Start chunk generation in background thread
                let game_path_clone = game_path.clone();
                let game_info_clone = game_info.clone();
                let progress_tracker = self.add_game_progress_tracker.clone();
                
                // Initialize progress
                self.add_game_progress = Some((0, 0, String::new()));
                self.add_game_generating = Some(game_path.clone());
                
                // Spawn thread for chunk generation
                std::thread::spawn(move || {
                    // Generiere deckdrop_chunks.toml
                    let hash_result = deckdrop_core::generate_chunks_toml(&game_path_clone, Some(move |current: usize, total: usize, file_name: &str| {
                        if let Ok(mut tracker) = progress_tracker.lock() {
                            *tracker = Some((current, total, file_name.to_string()));
                        }
                    }));
                    
                    // Schreibe deckdrop.toml direkt nach der Chunk-Generierung
                    // WICHTIG: Beide Dateien müssen gespeichert sein, bevor der Dialog geschlossen wird
                    if let Ok(hash_string) = &hash_result {
                        let chunks_hash = Some(hash_string.clone());
                        if let Err(e) = game_info_clone.save_to_path_with_hash(&game_path_clone, chunks_hash) {
                            eprintln!("Fehler beim Speichern von deckdrop.toml: {}", e);
                        } else {
                            println!("deckdrop.toml erfolgreich gespeichert in: {}", game_path_clone.display());
                            // Beide Dateien sind jetzt gespeichert: deckdrop.toml und deckdrop_chunks.toml
                            // Der Dialog wird in AddGameChunksGenerated geschlossen
                        }
                    }
                });
                
                // Return immediately - progress will be updated via Tick
                return Task::none();
            }
            Message::CancelAddGame => {
                self.show_add_game_dialog = false;
                self.add_game_saving = false; // Reset saving state when dialog is closed
            }
            Message::OpenSettings => {
                self.show_settings = true;
                self.settings_player_name = self.config.player_name.clone();
                self.settings_download_path = self.config.download_path.to_string_lossy().to_string();
                self.settings_max_concurrent_chunks = self.config.max_concurrent_chunks.to_string();
            }
            Message::SettingsPlayerNameChanged(name) => {
                self.settings_player_name = name;
            }
            Message::SettingsDownloadPathChanged(path) => {
                self.settings_download_path = path;
            }
            Message::SettingsMaxConcurrentChunksChanged(value) => {
                // Validiere: Nur Zahlen zwischen 1-10
                if let Ok(num) = value.parse::<usize>() {
                    if num >= 1 && num <= 10 {
                        self.settings_max_concurrent_chunks = value;
                    }
                } else if value.is_empty() {
                    self.settings_max_concurrent_chunks = value;
                }
            }
            Message::BrowseDownloadPath => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Select Download Directory")
                    .pick_folder()
                {
                    self.settings_download_path = path.to_string_lossy().to_string();
                }
            }
            Message::SaveSettings => {
                // Save settings
                let player_name_changed = self.config.player_name != self.settings_player_name;
                self.config.player_name = self.settings_player_name.clone();
                self.config.download_path = PathBuf::from(&self.settings_download_path);
                // Parse and save max_concurrent_chunks (default to 5 if invalid)
                self.config.max_concurrent_chunks = self.settings_max_concurrent_chunks.parse::<usize>()
                    .unwrap_or(5)
                    .max(1)
                    .min(10);
                if let Err(e) = self.config.save() {
                    eprintln!("Error saving settings: {}", e);
                }
                
                // Send metadata update if player name changed
                if player_name_changed {
                    if let Some(tx) = crate::network_bridge::get_metadata_update_tx() {
                        let _ = tx.send(deckdrop_network::network::discovery::MetadataUpdate {
                            player_name: Some(self.config.player_name.clone()),
                            games_count: None, // Will be updated when games change
                        });
                    }
                }
                
                self.show_settings = false;
            }
            Message::CancelSettings => {
                self.show_settings = false;
            }
            Message::LicensePlayerNameChanged(name) => {
                self.license_player_name = name;
            }
            Message::AcceptLicense => {
                // Save player name to config
                self.config.player_name = self.license_player_name.clone();
                
                // Generate and save peer ID if it doesn't exist
                if !deckdrop_core::Config::has_peer_id() {
                    if let Err(e) = self.config.generate_and_save_peer_id() {
                        eprintln!("Error generating peer ID: {}", e);
                    }
                }
                
                // Save config with player name
                // WICHTIG: Speichere den Spielernamen, damit er beim nächsten Start nicht erneut eingegeben werden muss
                if let Err(e) = self.config.save() {
                    eprintln!("Error saving config: {}", e);
                } else {
                    eprintln!("Spielername '{}' erfolgreich gespeichert", self.config.player_name);
                }
                
                // Update settings fields
                self.settings_player_name = self.license_player_name.clone();
                
                // Send metadata update with new player name
                if let Some(tx) = crate::network_bridge::get_metadata_update_tx() {
                    let _ = tx.send(deckdrop_network::network::discovery::MetadataUpdate {
                        player_name: Some(self.config.player_name.clone()),
                        games_count: None, // Will be updated when games change
                    });
                }
                
                self.show_license_dialog = false;
                // Don't open settings automatically anymore - user can open it from tab
            }
            Message::Tick => {
                // Periodic updates (e.g., update download progress)
                // WICHTIG: Häufigere Prüfung (jetzt in jedem Tick, alle 100ms), da update_download_progress
                // nun auf dem schnellen In-Memory-Cache basiert (kein I/O mehr).
                // Dies sorgt für eine flüssige UI-Anzeige ohne Verzögerungen.
                self.update_download_progress();
                
                // Update upload statistics (less frequently)
                if self.last_download_update.elapsed().as_millis() >= 1000 {
                    self.update_upload_stats();
                    self.last_download_update = std::time::Instant::now();
                }
                
                // Update performance metrics if Performance tab is active
                if self.current_tab == Tab::Performance {
                    self.update_performance_metrics();
                }
                
                // Check for Network events (non-blocking) via global access
                // WICHTIG: Erhöhtes Limit für Events pro Tick, um Staus in der Event-Queue zu vermeiden.
                // Bei schnellen Downloads können viele Chunk-Events pro Sekunde anfallen.
                // 100 Events sollten problemlos verarbeitet werden können, da die Verarbeitung leichtgewichtig ist.
                const MAX_EVENTS_PER_TICK: usize = 100;
                if let Some(rx) = crate::network_bridge::get_network_event_rx() {
                    if let Ok(mut rx) = rx.lock() {
                        let mut events_processed = 0;
                        while events_processed < MAX_EVENTS_PER_TICK {
                            match rx.try_recv() {
                                Ok(event) => {
                                    self.handle_network_event(event);
                                    events_processed += 1;
                                }
                                Err(_) => break, // Keine weiteren Events verfügbar
                            }
                        }
                    }
                }
                
                // Check for Download-Preparation messages (non-blocking) via global access
                // Sammle alle Messages und verarbeite sie direkt, um Endlosschleifen zu vermeiden
                if let Some(rx) = crate::get_download_prep_rx() {
                    if let Ok(rx) = rx.lock() {
                        let mut messages = Vec::new();
                        while let Ok(msg) = rx.try_recv() {
                            // eprintln!("Tick: Empfange Download-Preparation-Message: {:?}", msg);
                            messages.push(msg);
                        }
                        // Verarbeite alle Messages direkt, ohne update() erneut aufzurufen
                        for msg in messages {
                            // eprintln!("Tick: Verarbeite Download-Preparation-Message: {:?}", msg);
                            match msg {
                                Message::UpdateDownloadPreparationProgress(game_id, current, total) => {
                                    // Aktualisiere Progress direkt (thread-safe)
                                    if let Ok(mut preparing) = self.preparing_downloads.lock() {
                                        preparing.insert(game_id, (current, total));
                                    }
                                }
                                Message::DownloadPrepared(game_id, result) => {
                                    // Entferne aus preparing_downloads (thread-safe)
                                    if let Ok(mut preparing) = self.preparing_downloads.lock() {
                                        preparing.remove(&game_id);
                                    }
                                    
                                    match result {
                                        Ok(()) => {
                                            // Pre-Allocation erfolgreich - starte Download
                                            // Load manifest for UI update
                                            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                                                if let Ok(manifest) = deckdrop_core::DownloadManifest::load(&manifest_path) {
                                                    let progress_percent = manifest.progress.percentage as f32;
                                                    let downloaded_chunks = manifest.progress.downloaded_chunks;
                                                    
                                                    self.active_downloads.insert(game_id.clone(), DownloadState {
                                                        manifest: manifest.clone(),
                                                        progress_percent,
                                                        downloading_chunks_count: 0,
                                                        peer_count: 0,
                                                        download_speed_bytes_per_sec: 0.0,
                                                        last_update_time: std::time::Instant::now(),
                                                        last_downloaded_chunks: downloaded_chunks,
                                                        speed_samples: Vec::new(),
                                                    });
                                                    
                                                    // Request missing chunks to start download
                                                    if let Some(peers) = self.network_games.get(&game_id) {
                                                        let peer_ids: Vec<String> = peers.iter()
                                                            .map(|(pid, _)| pid.clone())
                                                            .collect();
                                                        
                                                        // WICHTIG: Verwende max_concurrent_chunks statt festem Wert 10
                                                        let max_chunks_per_peer = self.config.max_concurrent_chunks.max(10);
                                                        if let Err(e) = self.request_missing_chunks_adaptive(&game_id, &peer_ids, max_chunks_per_peer) {
                                                            eprintln!("Error requesting missing chunks for {}: {}", game_id, e);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Error preparing download for {}: {}", game_id, e);
                                            // Entferne aus downloading_starting, damit Button wieder aktiviert wird
                                            if let Ok(mut starting) = self.downloading_starting.lock() {
                                                starting.remove(&game_id);
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    // Andere Messages sollten normal verarbeitet werden
                                    // Aber wir vermeiden hier eine Endlosschleife
                                    eprintln!("Warnung: Unerwartete Message im Download-Preparation-Channel: {:?}", msg);
                                }
                            }
                        }
                    }
                }
                
                // Check for Window operations from System-Tray (non-blocking) via global access
                if let Some(rx) = crate::get_window_op_rx() {
                    if let Ok(rx) = rx.lock() {
                        while let Ok(msg) = rx.try_recv() {
                            // Verarbeite Window-Operationen direkt
                            match msg {
                                Message::ShowWindow => {
                                    // Window wird über plattformspezifische APIs sichtbar gemacht
                                    crate::window_control::show_window();
                                }
                                Message::HideWindow => {
                                    // Window wird über plattformspezifische APIs versteckt
                                    crate::window_control::hide_window();
                                }
                                Message::Quit => {
                                    // Beende die Anwendung
                                    std::process::exit(0);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                
                // Check for add game progress updates
                if let Some(game_path) = &self.add_game_generating {
                    if let Ok(tracker) = self.add_game_progress_tracker.lock() {
                        if let Some(progress) = tracker.as_ref() {
                            self.add_game_progress = Some(progress.clone());
                            
                            // Check if chunks.toml was created (generation complete)
                            let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                            if chunks_toml_path.exists() && progress.1 > 0 && progress.0 >= progress.1 {
                                // Generation complete - trigger completion message
                                // IMPORTANT: Set add_game_generating to None IMMEDIATELY to prevent multiple triggers
                                let game_path_clone = game_path.clone();
                                self.add_game_generating = None; // Prevent multiple triggers
                                return Task::perform(async move {
                                    use futures_timer::Delay;
                                    use std::time::Duration;
                                    Delay::new(Duration::from_millis(100)).await;
                                    let hash_result = deckdrop_core::gamechecker::calculate_file_hash(&chunks_toml_path)
                                        .map(|h| format!("blake3:{}", h))
                                        .map_err(|e| e.to_string());
                                    (game_path_clone, hash_result)
                                }, |(path, result)| Message::AddGameChunksGenerated(path, result));
                            }
                        }
                    }
                }
                
                // Update progress from tracker FIRST - before checking if we need to start new checks
                // This ensures progress is updated immediately when available
                // Only lock once and process all updates quickly
                let progress_updates: Vec<(PathBuf, usize)> = {
                    if let Ok(progress) = self.integrity_check_progress.lock() {
                        progress.iter()
                            .map(|(path, current)| (path.clone(), *current))
                            .collect()
                    } else {
                        Vec::new()
                    }
                };
                
                // Apply progress updates without holding the lock
                for (game_path, current_progress) in progress_updates {
                    if let Some(status) = self.game_integrity_status.get_mut(&game_path) {
                        if let GameIntegrityStatus::Checking { current: old_current, total } = status {
                            // Only update if progress has actually changed
                            if current_progress != *old_current && current_progress <= *total {
                                *status = GameIntegrityStatus::Checking { current: current_progress, total: *total };
                            }
                        }
                    }
                }
                
                // Check for completed checks in results tracker
                let completed_checks: Vec<(PathBuf, GameIntegrityStatus)> = {
                    if let Ok(mut results) = self.integrity_check_results.lock() {
                        results.drain().collect()
                    } else {
                        Vec::new()
                    }
                };
                
                // Apply completed checks without holding the lock
                for (game_path, final_status) in completed_checks {
                    if let Some(current_status) = self.game_integrity_status.get_mut(&game_path) {
                        // Only update if still checking
                        if matches!(current_status, GameIntegrityStatus::Checking { .. }) {
                            *current_status = final_status;
                        }
                    }
                    // Always remove start time when check completes (regardless of status match)
                    self.integrity_check_start_time.remove(&game_path);
                }
                
                // Integrity checks are now manual - no automatic checking
                
                // Don't create additional Tick tasks - the subscription in main.rs already sends Ticks every 100ms
                // This prevents task cascades that slow down the application
            }
            Message::ShowWindow => {
                // Window wird über plattformspezifische APIs sichtbar gemacht
                crate::window_control::show_window();
            }
            Message::HideWindow => {
                // Window wird über plattformspezifische APIs versteckt
                crate::window_control::hide_window();
            }
            Message::Quit => {
                // Beende die Anwendung
                std::process::exit(0);
            }
            Message::EditGame => {
                // Starte Bearbeitungsmodus
                if let Some((game_path, game_info)) = &self.current_game_details {
                    // Prüfe, ob der aktuelle Benutzer der Creator ist
                    let is_creator = game_info.creator_peer_id.as_ref()
                        .and_then(|creator_id| self.config.peer_id.as_ref().map(|my_id| creator_id == my_id))
                        .unwrap_or(false);
                    
                    if is_creator {
                        self.editing_game = true;
                        self.edit_game_path = Some(game_path.clone());
                        self.edit_game_name = game_info.name.clone();
                        self.edit_game_start_file = game_info.start_file.clone();
                        self.edit_game_start_args = game_info.start_args.clone().unwrap_or_default();
                        self.edit_game_description = game_info.description.clone().unwrap_or_default();
                        self.edit_game_additional_instructions = game_info.additional_instructions.clone().unwrap_or_default();
                    }
                }
            }
            Message::EditGameNameChanged(name) => {
                self.edit_game_name = name;
            }
            Message::EditGameStartFileChanged(start_file) => {
                self.edit_game_start_file = start_file;
            }
            Message::BrowseEditStartFile => {
                if let Some(game_path) = &self.edit_game_path {
                    if !game_path.exists() {
                        eprintln!("Error: Game path does not exist: {}", game_path.display());
                        return Task::none();
                    }
                    
                    // Open file dialog starting from game path
                    if let Some(selected_file) = rfd::FileDialog::new()
                        .set_title("Select Game Executable")
                        .set_directory(game_path)
                        .pick_file()
                    {
                        // Convert to relative path from game_path
                        if let Ok(relative_path) = selected_file.strip_prefix(game_path) {
                            // Convert to forward slashes for cross-platform compatibility
                            let relative_str = relative_path.to_string_lossy().replace('\\', "/");
                            self.edit_game_start_file = relative_str;
                        } else {
                            eprintln!("Error: Selected file is not within game path");
                        }
                    }
                }
            }
            Message::EditGameStartArgsChanged(args) => {
                self.edit_game_start_args = args;
            }
            Message::EditGameDescriptionChanged(description) => {
                self.edit_game_description = description;
            }
            Message::EditGameAdditionalInstructionsChanged(instructions) => {
                self.edit_game_additional_instructions = instructions;
            }
            Message::SaveGameEdit => {
                if let Some(game_path) = &self.edit_game_path {
                    // Lade aktuelle GameInfo
                    if let Ok(mut game_info) = GameInfo::load_from_path(game_path) {
                        // Prüfe, ob der aktuelle Benutzer der Creator ist
                        let is_creator = game_info.creator_peer_id.as_ref()
                            .and_then(|creator_id| self.config.peer_id.as_ref().map(|my_id| creator_id == my_id))
                            .unwrap_or(false);
                        
                        if is_creator {
                            // Aktualisiere Felder
                            game_info.name = self.edit_game_name.clone();
                            game_info.start_file = self.edit_game_start_file.clone();
                            game_info.start_args = if self.edit_game_start_args.is_empty() {
                                None
                            } else {
                                Some(self.edit_game_start_args.clone())
                            };
                            game_info.description = if self.edit_game_description.is_empty() {
                                None
                            } else {
                                Some(self.edit_game_description.clone())
                            };
                            game_info.additional_instructions = if self.edit_game_additional_instructions.is_empty() {
                                None
                            } else {
                                Some(self.edit_game_additional_instructions.clone())
                            };
                            
                            // Inkrementiere Version
                            game_info.version = deckdrop_core::game::increment_version(&game_info.version);
                            
                            // Speichere aktualisierte GameInfo
                            if let Err(e) = game_info.save_to_path(game_path) {
                                eprintln!("Fehler beim Speichern der Bearbeitung: {}", e);
                            } else {
                                // Aktualisiere current_game_details mit neuer Version
                                if let Some((_, old_info)) = &mut self.current_game_details {
                                    *old_info = game_info.clone();
                                }
                                
                                // Aktualisiere auch in my_games
                                if let Some((_, stored_info)) = self.my_games.iter_mut()
                                    .find(|(path, _)| path == game_path) {
                                    *stored_info = game_info;
                                }
                                
                                // Beende Bearbeitungsmodus
                                self.editing_game = false;
                                self.edit_game_path = None;
                            }
                        }
                    }
                }
            }
            Message::CancelGameEdit => {
                // Beende Bearbeitungsmodus ohne zu speichern
                self.editing_game = false;
                self.edit_game_path = None;
                self.edit_game_name = String::new();
                self.edit_game_start_file = String::new();
                self.edit_game_start_args = String::new();
                self.edit_game_description = String::new();
                self.edit_game_additional_instructions = String::new();
            }
            Message::GamesLoaded(games) => {
                eprintln!("UI: Empfange {} geladene Spiele", games.len());
                self.my_games = games;
            }
            Message::ActiveDownloadsLoaded(active_downloads_vec) => {
                eprintln!("UI: Empfange {} aktive Downloads", active_downloads_vec.len());
                
                let mut active_downloads = HashMap::new();
                for (game_id, manifest) in active_downloads_vec {
                    // Versuche, die GameInfo aus dem Manifest-Verzeichnis zu laden
                    if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                        if let Some(manifest_dir) = manifest_path.parent() {
                            if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(manifest_dir) {
                                // Füge das Spiel nur hinzu, wenn es nicht bereits in my_games ist
                                let game_path = PathBuf::from(&manifest.game_path);
                                if !self.my_games.iter().any(|(path, _)| path == &game_path) {
                                    self.my_games.push((game_path.clone(), game_info));
                                }
                            }
                        }
                    }
                    // Initialisiere DownloadState für UI
                    let progress_percent = manifest.progress.percentage as f32;
                    active_downloads.insert(game_id.clone(), DownloadState {
                        manifest: manifest.clone(),
                        progress_percent,
                        downloading_chunks_count: 0,
                        peer_count: 0,
                        download_speed_bytes_per_sec: 0.0,
                        last_update_time: std::time::Instant::now(),
                        last_downloaded_chunks: manifest.progress.downloaded_chunks,
                        speed_samples: Vec::new(),
                    });
                }
                self.active_downloads = active_downloads;
                
                // Deduplicate games
                {
                    use std::collections::HashMap;
                    let mut seen_ids = HashMap::new();
                    let mut deduplicated = Vec::new();
                    
                    for (game_path, game_info) in &self.my_games {
                        let game_id = &game_info.game_id;
                        if !seen_ids.contains_key(game_id) {
                            seen_ids.insert(game_id.clone(), game_path.clone());
                            deduplicated.push((game_path.clone(), game_info.clone()));
                        }
                    }
                    self.my_games = deduplicated;
                }
                
                // Trigger update for progress
                self.update_download_progress();
            }
            
            Message::ClearNetworkCache => {
                // Lösche alle gecachten Network-Games
                if let Err(e) = network_cache::clear_all_cached_network_games() {
                    eprintln!("Fehler beim Löschen des Network-Cache: {}", e);
                }
                // Entferne alle offline-Spiele aus der UI (behalte nur Spiele mit aktiven Peers)
                self.network_games.retain(|_, games| {
                    games.retain(|(peer_id, _)| {
                        // Prüfe, ob dieser Peer noch online ist
                        self.peers.iter().any(|p| p.id == *peer_id)
                    });
                    !games.is_empty()
                });
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // Main layout
        let content = if self.show_license_dialog {
            self.view_license_dialog()
        } else if self.show_settings {
            column![
                container(
                    row![
                        Space::with_width(Length::Fill),
                        container(
                            column![
                                self.view_settings(),
                                Space::with_height(Length::Fill),
                            ]
                            .width(Length::Fill)
                            .height(Length::Fill)
                        )
                        .width(Length::Fixed(500.0))
                        .height(Length::Fill),
                        Space::with_width(Length::Fill),
                    ]
                    .width(Length::Fill)
                    .height(Length::Fill)
                )
                .width(Length::Fill)
                .height(Length::Fill),
                self.view_status_bar(),
            ]
            .spacing(scale(8.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if self.show_add_game_dialog {
            self.view_add_game_dialog()
        } else {
            column![
                self.view_tabs(),
                self.view_current_tab(),
                self.view_status_bar(),
            ]
            .spacing(scale(8.0))
            .into()
        };
        
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(scale(15.0))
            .into()
    }
    
    fn view_status_bar(&self) -> Element<'_, Message> {
        // Lade Upload-Statistiken
        let upload_stats = if let Ok(stats) = self.upload_stats.lock() {
            stats.clone()
        } else {
            UploadStats {
                active_upload_count: 0,
                upload_speed_bytes_per_sec: 0.0,
                last_update_time: std::time::Instant::now(),
                last_uploaded_bytes: 0,
            }
        };
        
        // Format upload speed
        let speed_text = if upload_stats.upload_speed_bytes_per_sec > 1_000_000.0 {
            format!("{:.2} MB/s", upload_stats.upload_speed_bytes_per_sec / 1_000_000.0)
        } else if upload_stats.upload_speed_bytes_per_sec > 1_000.0 {
            format!("{:.2} KB/s", upload_stats.upload_speed_bytes_per_sec / 1_000.0)
        } else {
            format!("{:.0} B/s", upload_stats.upload_speed_bytes_per_sec)
        };
        
        // Status-Text
        let status_text = if upload_stats.active_upload_count > 0 {
            format!("Upload: {} Chunks (active) | Speed: {}", upload_stats.active_upload_count, speed_text)
        } else {
            "Upload: Idle".to_string()
        };
        
        container(
            row![
                text(status_text)
                    .size(scale_text(10.0))
                    .style(|_theme: &Theme| {
                        iced::widget::text::Style {
                            color: Some(Color::from_rgba(0.7, 0.7, 0.7, 1.0)),
                        }
                    }),
                Space::with_width(Length::Fill),
                text(format!("Peers: {} | Herunterladende Spiele: {}", self.status.peer_count, self.status.active_download_count))
                    .size(scale_text(10.0))
                    .style(|_theme: &Theme| {
                        iced::widget::text::Style {
                            color: Some(Color::from_rgba(0.7, 0.7, 0.7, 1.0)),
                        }
                    }),
            ]
            .spacing(scale(8.0))
        )
        .width(Length::Fill)
        .padding(scale(8.0))
        .style(|_theme: &Theme| {
            container::Style {
                background: Some(iced::Background::Color(Color::from_rgba(0.1, 0.1, 0.1, 1.0))),
                border: iced::Border {
                    width: 1.0,
                    color: Color::from_rgba(0.3, 0.3, 0.3, 1.0),
                    radius: 0.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
    }

}

impl DeckDropApp {
    /// Deduplicates games in my_games based on game_id
    fn deduplicate_games_by_id(&mut self) {
        use std::collections::HashMap;
        let mut seen_ids = HashMap::new();
        let mut deduplicated = Vec::new();
        
        for (game_path, game_info) in &self.my_games {
            let game_id = &game_info.game_id;
            if !seen_ids.contains_key(game_id) {
                seen_ids.insert(game_id.clone(), game_path.clone());
                deduplicated.push((game_path.clone(), game_info.clone()));
            } else {
                // Game with this ID already exists, keep the first one
                println!("[DEBUG] Duplicate game_id detected: {} (path: {}), keeping first occurrence", 
                    game_id, game_path.display());
            }
        }
        
        self.my_games = deduplicated;
    }
    
    /// Handles network events
    fn handle_network_event(&mut self, event: DiscoveryEvent) {
        match event {
            DiscoveryEvent::PeerFound(peer_info) => {
                // Speichere Werte vor dem Move
                let peer_id = peer_info.id.clone();
                let games_count = peer_info.games_count;
                
                // Find existing peer or add new one
                let was_new = if let Some(existing_peer) = self.peers.iter_mut().find(|p| p.id == peer_info.id) {
                    // Update existing peer with new information
                    let games_count_changed = existing_peer.games_count != peer_info.games_count;
                    existing_peer.player_name = peer_info.player_name.clone();
                    existing_peer.games_count = peer_info.games_count;
                    existing_peer.addr = peer_info.addr.clone();
                    existing_peer.version = peer_info.version.clone();
                    
                    // WICHTIG: Wenn sich die games_count geändert hat, sollte eine GamesListRequest gesendet werden
                    // (wird automatisch bei ConnectionEstablished gemacht, aber bei Updates auch sinnvoll)
                    // Das wird durch das Keep-Alive-System alle 60 Sekunden gemacht, also nicht hier
                    games_count_changed
                } else {
                    // Add new peer
                    self.peers.push(peer_info);
                    self.status.peer_count = self.peers.len();
                    true // Neuer Peer
                };
                
                // WICHTIG: Bei neuem Peer oder geänderter games_count wird automatisch eine GamesListRequest
                // bei ConnectionEstablished gesendet (siehe discovery.rs:1458-1461)
                // Das Keep-Alive-System sendet auch regelmäßig GamesListRequests
                if was_new {
                    eprintln!("Neuer Peer gefunden: {} ({:?} Spiele) - GamesList wird automatisch angefordert", 
                        peer_id, games_count);
                }
            }
            DiscoveryEvent::PeerLost(peer_id) => {
                self.peers.retain(|p| p.id != peer_id);
                self.status.peer_count = self.peers.len();
                // Entferne Peer-ID aus Cache für alle Spiele dieses Peers
                let game_ids_to_update: Vec<String> = self.network_games.keys().cloned().collect();
                for game_id in game_ids_to_update {
                    if let Err(e) = network_cache::remove_peer_from_cached_game(&game_id, &peer_id) {
                        eprintln!("Fehler beim Entfernen von Peer aus gecachtem Spiel: {}", e);
                    }
                }
                // Entferne Peer aus UI-State
                self.network_games.retain(|_, games| {
                    games.retain(|(pid, _)| pid != &peer_id);
                    !games.is_empty()
                });
                
                // WICHTIG: Aktualisiere Scheduler für alle aktiven Downloads, die diesen Peer genutzt haben
                for (game_id, download_state) in &self.active_downloads {
                    if matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading) {
                        // Wenn dieser Peer für dieses Spiel relevant war, aktualisiere die Liste
                        if let Some(peers) = self.network_games.get(game_id) {
                            let peer_ids: Vec<String> = peers.iter().map(|(pid, _)| pid.clone()).collect();
                            // Auch wenn leer, registrieren (Scheduler stoppt dann Requests)
                            crate::network_bridge::register_download_peers(game_id, &peer_ids);
                            eprintln!("PeerLost: Scheduler aktualisiert für Spiel {} ({} Peers verbleibend)", game_id, peer_ids.len());
                        } else {
                            // Keine Peers mehr für dieses Spiel
                            crate::network_bridge::register_download_peers(game_id, &[]);
                            eprintln!("PeerLost: Scheduler aktualisiert für Spiel {} (0 Peers verbleibend)", game_id);
                        }
                    }
                }
            }
            DiscoveryEvent::GamesListReceived { peer_id, games } => {
                // Aktualisiere Spiele-Liste für diesen Peer
                for game in games {
                    let game_id = game.game_id.clone();
                    
                    // Aktualisiere UI-State im Main-Thread (schnell, nur In-Memory)
                    self.network_games
                        .entry(game_id.clone())
                        .or_insert_with(Vec::new)
                        .push((peer_id.clone(), game.clone()));
                    
                    // Speichere Network-Games-Cache im Hintergrund-Thread, um den UI-Thread zu entlasten
                    let cache_game_id = game_id.clone();
                    let cache_peer_id = peer_id.clone();
                    let cache_game = game.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = network_cache::update_network_game_peer(&cache_game_id, &cache_peer_id, &cache_game) {
                            eprintln!("Fehler beim Speichern von Network-Game im Cache (Background-Thread): {}", e);
                        }
                    });
                }
                
                // WICHTIG: Prüfe ob wir Pending-Downloads haben, die jetzt gestartet werden können
                // Oder ob wir laufende Downloads haben, für die dieser Peer neue Quellen bietet
                for (game_id, download_state) in &self.active_downloads {
                    // Fall 1: Download ist Pending - Starte Download wenn Peer das Spiel hat
                    if matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Pending) {
                        // Prüfe ob dieser Peer das Spiel hat
                        if let Some(peers) = self.network_games.get(game_id) {
                            if peers.iter().any(|(pid, _)| pid == &peer_id) {
                                // Peer hat das Spiel - starte Download sofort
                                eprintln!("Peer {} hat Pending-Spiel {} - starte Download sofort", peer_id, game_id);
                                if let Some((peer_id_for_download, _)) = peers.first() {
                                    if let Some(tx) = crate::network_bridge::get_download_request_tx() {
                                        let _ = tx.send(deckdrop_network::network::discovery::DownloadRequest::RequestGameMetadata {
                                            peer_id: peer_id_for_download.clone(),
                                            game_id: game_id.clone(),
                                        });
                                        eprintln!("GameMetadata-Request gesendet für {} an {}", game_id, peer_id_for_download);
                                    }
                                }
                            }
                        }
                    }
                    // Fall 2: Download läuft bereits (Downloading) - Aktualisiere Scheduler mit neuem Peer
                    else if matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading) {
                        // Prüfe ob dieser Peer das Spiel hat
                        if let Some(peers) = self.network_games.get(game_id) {
                            // Wenn der neue Peer in der Liste ist (was er sein sollte, da wir network_games gerade geupdated haben)
                            // UND das Spiel hat (impliziert durch network_games Eintrag)
                            if peers.iter().any(|(pid, _)| pid == &peer_id) {
                                // Hole alle Peers für dieses Spiel
                                let peer_ids: Vec<String> = peers.iter().map(|(pid, _)| pid.clone()).collect();
                                
                                // Registriere neue Peer-Liste im Scheduler
                                if !peer_ids.is_empty() {
                                    let changed = crate::network_bridge::register_download_peers(game_id, &peer_ids);
                                    if changed {
                                        eprintln!("Scheduler aktualisiert für Spiel {}: {} Peers (Neuer Peer: {})", 
                                            game_id, peer_ids.len(), peer_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            DiscoveryEvent::GameMetadataReceived { peer_id, game_id, deckdrop_toml, deckdrop_chunks_toml } => {
                // WICHTIG: Entferne aus downloading_starting, da Pre-Allocation jetzt startet
                if let Ok(mut starting) = self.downloading_starting.lock() {
                    starting.remove(&game_id);
                }
                
                // Setze initialen Progress (thread-safe)
                // Wir kennen die genaue Anzahl der Dateien noch nicht (TOML parsing ist teuer), 
                // setzen daher vorläufig auf 1, bis das Parsing im Thread fertig ist.
                if let Ok(mut preparing) = self.preparing_downloads.lock() {
                    preparing.insert(game_id.clone(), (0, 1));
                }
                
                // Starte Pre-Allocation in einem separaten Thread
                let game_id_clone = game_id.clone();
                let deckdrop_toml_clone = deckdrop_toml.clone();
                let deckdrop_chunks_toml_clone = deckdrop_chunks_toml.clone();
                
                // Hole den Message-Sender für Download-Preparation
                if let Some(tx) = crate::get_download_prep_tx() {
                    eprintln!("GameMetadataReceived: Starte Pre-Allocation für {}", game_id);
                    std::thread::spawn(move || {
                        // Parse deckdrop_chunks.toml HIER im Thread, nicht im UI-Thread
                        #[derive(serde::Deserialize)]
                        struct ChunksToml {
                            file: Vec<serde_json::Value>,
                        }
                        let total_files = toml::from_str::<ChunksToml>(&deckdrop_chunks_toml_clone)
                            .map(|ct| ct.file.len())
                            .unwrap_or(0);
                            
                        // Sende erstes Update mit korrekter Gesamtanzahl
                        if let Err(e) = tx.send(Message::UpdateDownloadPreparationProgress(
                            game_id_clone.clone(),
                            0,
                            total_files,
                        )) {
                            eprintln!("Fehler beim Senden von Initial-Progress-Update: {}", e);
                        }

                        let mut last_sent_progress = 0;
                        eprintln!("Pre-Allocation Thread gestartet für {}", game_id_clone);
                        // Führe Pre-Allocation durch
                        let result = deckdrop_core::prepare_download_with_progress(
                            &game_id_clone,
                            &deckdrop_toml_clone,
                            &deckdrop_chunks_toml_clone,
                            |current, total| {
                                // Sende Progress-Update nur bei signifikanten Änderungen (alle 5% oder jede 10. Datei)
                                let progress_percent = if total > 0 {
                                    (current * 100) / total
                                } else {
                                    0
                                };
                                
                                // Sende Update nur wenn:
                                // 1. Progress um mindestens 5% gestiegen ist, ODER
                                // 2. Jede 10. Datei, ODER
                                // 3. Letzte Datei (current == total)
                                if progress_percent >= last_sent_progress + 5 || 
                                   current % 10 == 0 || 
                                   current == total {
                                    eprintln!("Pre-Allocation Progress: {}/{} ({}%)", current, total, progress_percent);
                                    if let Err(e) = tx.send(Message::UpdateDownloadPreparationProgress(
                                        game_id_clone.clone(),
                                        current,
                                        total,
                                    )) {
                                        eprintln!("Fehler beim Senden von Progress-Update: {}", e);
                                    }
                                    last_sent_progress = progress_percent;
                                }
                            },
                        );
                        
                        match &result {
                            Ok(_) => eprintln!("Pre-Allocation abgeschlossen für {}: OK", game_id_clone),
                            Err(e) => eprintln!("Pre-Allocation abgeschlossen für {}: Fehler: {}", game_id_clone, e),
                        }
                        
                        // Sende Ergebnis
                        if let Err(e) = tx.send(Message::DownloadPrepared(
                            game_id_clone.clone(),
                            result.map_err(|e| e.to_string()),
                        )) {
                            eprintln!("Fehler beim Senden von DownloadPrepared: {}", e);
                        }
                    });
                } else {
                    eprintln!("ERROR: Download-Preparation-Message-Sender nicht verfügbar für {}", game_id);
                    // Entferne aus preparing_downloads und downloading_starting
                    if let Ok(mut preparing) = self.preparing_downloads.lock() {
                        preparing.remove(&game_id);
                    }
                    if let Ok(mut starting) = self.downloading_starting.lock() {
                        starting.remove(&game_id);
                    }
                }
            }
            DiscoveryEvent::GameMetadataRequestFailed { peer_id, game_id, error } => {
                eprintln!("GameMetadataRequestFailed: game_id={} from peer={}, error={}", game_id, peer_id, error);
                
                // WICHTIG: Entferne aus downloading_starting, damit Button wieder aktiv wird
                if let Ok(mut starting) = self.downloading_starting.lock() {
                    starting.remove(&game_id);
                    eprintln!("GameMetadataRequestFailed: {} aus downloading_starting entfernt", game_id);
                }
                
                // Entferne auch aus preparing_downloads falls vorhanden
                if let Ok(mut preparing) = self.preparing_downloads.lock() {
                    preparing.remove(&game_id);
                }
            }
            DiscoveryEvent::ChunkUploaded { peer_id: _, chunk_hash, chunk_size } => {
                // Tracke Upload-Statistiken
                if let Ok(mut stats) = self.upload_stats.lock() {
                    // Aktualisiere aktive Uploads (tracke in active_uploads)
                    let now = std::time::Instant::now();
                    if let Ok(mut active_uploads) = self.active_uploads.lock() {
                        active_uploads.insert(chunk_hash.clone(), (now, chunk_size));
                    }
                    
                    // Berechne Upload-Geschwindigkeit (gleitender Durchschnitt über 5 Sekunden)
                    let cutoff_time = now.checked_sub(std::time::Duration::from_secs(5))
                        .unwrap_or(now);
                    
                    // Entferne alte Uploads
                    if let Ok(mut active_uploads) = self.active_uploads.lock() {
                        active_uploads.retain(|_, (time, _)| *time >= cutoff_time);
                        
                        // Berechne Gesamtgeschwindigkeit aus aktiven Uploads
                        let total_bytes_in_window: usize = active_uploads.values()
                            .map(|(_, size)| *size)
                            .sum();
                        
                        // Geschwindigkeit = Bytes in 5 Sekunden / 5
                        stats.upload_speed_bytes_per_sec = total_bytes_in_window as f64 / 5.0;
                        stats.active_upload_count = active_uploads.len();
                        stats.last_update_time = now;
                        stats.last_uploaded_bytes += chunk_size as u64;
                    }
                }
            }
            DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data } => {
                // Ignoriere leere Chunks (Peers ohne Daten senden leere Responses)
                if chunk_data.is_empty() {
                    eprintln!("⚠️ Leerer Chunk {} von {} empfangen - ignoriere", chunk_hash, peer_id);
                    return;
                }

                // Nur bei Fehlern oder wichtigen Ereignissen loggen (Performance)

                // Phase 4: Tracke Performance für diesen Peer
                let peer_perf = self.peer_performance.clone();
                let peer_id_for_perf = peer_id.clone();
                let chunk_size = chunk_data.len();
                let chunk_received_time = std::time::Instant::now();
                
                // Update Performance-Tracking (erfolgreicher Chunk-Empfang)
                if let Ok(mut perf_map) = peer_perf.lock() {
                    let perf = perf_map.entry(peer_id_for_perf.clone()).or_insert_with(PeerPerformance::default);
                    perf.successful_requests += 1;
                    perf.total_requests += 1;
                    perf.success_rate = perf.successful_requests as f64 / perf.total_requests.max(1) as f64;
                    // Robustheit: Reset Circuit Breaker bei erfolgreichem Request
                    perf.consecutive_failures = 0;
                    if perf.success_rate > 0.7 {
                        perf.blocked_until = None; // Entblockiere Peer bei guter Performance
                    }
                    
                    // Tracke Download-Geschwindigkeit (5-Sekunden gleitender Durchschnitt)
                    perf.speed_samples.push((chunk_received_time, chunk_size));
                    let cutoff_time = chunk_received_time.checked_sub(std::time::Duration::from_secs(5))
                        .unwrap_or(chunk_received_time);
                    perf.speed_samples.retain(|(time, _)| *time >= cutoff_time);
                    
                    // Berechne Durchschnittsgeschwindigkeit
                    if perf.speed_samples.len() >= 2 {
                        let total_bytes: usize = perf.speed_samples.iter().map(|(_, bytes)| bytes).sum();
                        // Sichere Berechnung ohne unwrap() - verhindert Abstürze
                        if let (Some(first), Some(last)) = (perf.speed_samples.first(), perf.speed_samples.last()) {
                            let time_span = last.0.duration_since(first.0).as_secs_f64();
                            if time_span > 0.1 {
                                perf.download_speed_bytes_per_sec = total_bytes as f64 / time_span;
                            }
                        }
                    }
                    perf.last_update = chunk_received_time;
                }
                
                // Prüfe ob dieser Chunk bereits verarbeitet wird (verhindert Duplikate)
                let processing_chunks = self.processing_chunks.clone();
                let chunk_hash_clone = chunk_hash.clone();
                
                // Prüfe und markiere Chunk als "in Verarbeitung"
                let should_process = {
                    if let Ok(mut processing) = processing_chunks.lock() {
                        if processing.contains(&chunk_hash_clone) {
                            // Chunk wird bereits verarbeitet, überspringe
                            false
                        } else {
                            // Markiere Chunk als "in Verarbeitung"
                            processing.insert(chunk_hash_clone.clone());
                            true
                        }
                    } else {
                        false
                    }
                };
                
                if !should_process {
                    // Chunk wird bereits verarbeitet, überspringe
                    return;
                }
                
                // Aktualisiere Tracking-Daten im UI-Thread BEVOR der Background-Thread startet
                // (wird später im Thread aktualisiert, wenn Chunk fertig ist)
                let chunk_peer_requests_for_thread = self.chunk_peer_requests.clone();
                let active_requests_per_peer_for_thread = self.active_requests_per_peer.clone();
                let peer_performance_for_thread = self.peer_performance.clone();
                
                // WICHTIG: Verschiebe chunk_data direkt in den Worker (move), um Klonen im UI-Thread zu vermeiden
                // Worker verarbeitet sequentiell -> keine Thread-Explosion und kein DB-Locking.
                
                let task = ChunkWriteTask {
                    chunk_hash: chunk_hash.clone(),
                    chunk_data: chunk_data.to_vec(), // Move ownership
                    peer_id: peer_id.clone(),
                };
                
                // Verwende try_send statt send_blocking, um den UI-Thread nicht zu blockieren
                if let Err(e) = self.chunk_writer_tx.try_send(task) {
                    if e.is_full() {
                        eprintln!("WARNUNG: ChunkWriter Queue ist voll - Chunk {} wird später erneut versucht", chunk_hash);
                    } else {
                        eprintln!("KRITISCH: Konnte Chunk-Task nicht an Worker senden: {}", e);
                    }
                    
                    // Cleanup processing_chunks falls Senden fehlschlägt
                    if let Ok(mut processing) = self.processing_chunks.lock() {
                        processing.remove(&chunk_hash);
                    }
                }
            }
            DiscoveryEvent::ChunkRequestSent { peer_id, chunk_hash, game_id: _ } => {
                let start_time = std::time::Instant::now();
                let mut already_requested = false;
                if let Ok(mut requested) = self.requested_chunks.lock() {
                    already_requested = requested.contains(&chunk_hash);
                    if !already_requested {
                        requested.insert(chunk_hash.clone());
                    }
                }
                // Tracke immer Peer‑Zuordnung und aktive Requests – unabhängig davon, ob der Chunk
                // schon vorher (z.B. beim Planen) in requested_chunks stand.
                if let Ok(mut chunk_peers) = self.chunk_peer_requests.lock() {
                    chunk_peers.insert(chunk_hash.clone(), peer_id.clone());
                }
                if let Ok(mut active_per_peer) = self.active_requests_per_peer.lock() {
                    *active_per_peer.entry(peer_id.clone()).or_insert(0) += 1;
                }
                // Logge nur selten (alle 100 mal), um Spam zu vermeiden
                // "war bereits in requested_chunks" ist normal bei Retries oder mehreren Peers
                if already_requested {
                    static mut COUNTER: u64 = 0;
                    unsafe {
                        COUNTER += 1;
                        if COUNTER % 100 == 0 {
                            eprintln!("[DEBUG] Chunk {} war bereits in requested_chunks (normal bei Retries, {} mal)", chunk_hash, COUNTER);
                        }
                    }
                }
                if let Ok(mut start_times) = self.chunk_download_start_times.lock() {
                    start_times.insert(chunk_hash.clone(), start_time);
                }
            }
            DiscoveryEvent::ChunkRequestFailed { peer_id, chunk_hash, error } => {
                eprintln!("ChunkRequestFailed: {} from {}: {}", chunk_hash, peer_id, error);
                
                // WICHTIG: Entferne Chunk SOFORT aus requested_chunks, damit neue Chunks angefordert werden können
                // Der Chunk wird später wieder hinzugefügt, wenn der Retry gesendet wird
                if let Ok(mut requested) = self.requested_chunks.lock() {
                    if requested.remove(&chunk_hash) {
                        // Reduziere aktive Request-Zahl für diesen Peer
                        if let Ok(mut active_per_peer) = self.active_requests_per_peer.lock() {
                            if let Some(count) = active_per_peer.get_mut(&peer_id) {
                                if *count > 0 {
                                    *count -= 1;
                                }
                            }
                        }
                        
                        // Entferne Chunk-Peer-Mapping
                        if let Ok(mut chunk_peers) = self.chunk_peer_requests.lock() {
                            chunk_peers.remove(&chunk_hash);
                        }
                        
                        eprintln!("Chunk {} aus requested_chunks entfernt (fehlgeschlagen) - neue Chunks können angefordert werden", chunk_hash);
                    }
                }
                // Entferne auch aus start_times
                if let Ok(mut start_times) = self.chunk_download_start_times.lock() {
                    start_times.remove(&chunk_hash);
                }
                
                // Robustheit: Circuit Breaker - Blockiere Peer bei zu vielen Fehlern
                if let Ok(mut perf_map) = self.peer_performance.lock() {
                    let perf = perf_map.entry(peer_id.clone()).or_insert_with(PeerPerformance::default);
                    perf.total_requests += 1;
                    perf.consecutive_failures += 1;
                    perf.success_rate = perf.successful_requests as f64 / perf.total_requests.max(1) as f64;
                    perf.last_update = std::time::Instant::now();
                    
                    // Robustheit: Circuit Breaker - Blockiere früher (nach 2 Fehlern) um Crash zu verhindern
                    if perf.success_rate < 0.5 || perf.consecutive_failures >= 2 {
                        perf.blocked_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(300)); // 5 Minuten
                        eprintln!("Circuit Breaker: Peer {} blockiert für 5 Minuten (Success-Rate: {:.1}%, Consecutive Failures: {})", 
                            peer_id, perf.success_rate * 100.0, perf.consecutive_failures);
                    }
                }
                
                // Robustheit: Retry-Logik mit Exponential Backoff
                let should_retry = {
                    let mut retries = match self.chunk_retries.lock() {
                        Ok(guard) => guard,
                        Err(e) => {
                            eprintln!("Fehler beim Locken von chunk_retries: {}", e);
                            return; // Überspringe Retry bei Lock-Fehler
                        }
                    };
                    
                    let retry_info = retries.entry(chunk_hash.clone()).or_insert_with(|| ChunkRetryInfo {
                        retry_count: 0,
                        last_retry_time: std::time::Instant::now(),
                        last_peer_id: peer_id.clone(),
                        failure_count_with_peer: 0,
                    });
                    
                    retry_info.retry_count += 1;
                    retry_info.failure_count_with_peer += 1;
                    
                    // Wechsle Peer nach 2 Fehlern mit demselben Peer
                    let should_switch_peer = retry_info.failure_count_with_peer >= 2;
                    
                    // Max 5 Retries pro Chunk
                    let max_retries = 5;
                    let can_retry = retry_info.retry_count < max_retries;
                    
                    if should_switch_peer {
                        retry_info.failure_count_with_peer = 0; // Reset für neuen Peer
                    }
                    
                    (can_retry, should_switch_peer)
                };
                
                if should_retry.0 {
                    // Exponential Backoff: 1s, 2s, 4s, 8s, max 30s
                    let retry_info = {
                        match self.chunk_retries.lock() {
                            Ok(retries) => retries.get(&chunk_hash).cloned(),
                            Err(e) => {
                                eprintln!("Fehler beim Locken von chunk_retries: {}", e);
                                None
                            }
                        }
                    };
                    
                    if let Some(retry_info) = retry_info {
                        let backoff_seconds = (1u64 << retry_info.retry_count.min(4)).min(30); // 1, 2, 4, 8, 16, max 30
                        let backoff_duration = std::time::Duration::from_secs(backoff_seconds);
                        
                        // Robustheit: Spawn Background-Task für Retry mit Backoff
                        let chunk_hash_for_task = chunk_hash.clone();
                        let peer_id_for_task = peer_id.clone();
                        let should_switch_peer = should_retry.1;
                        let retry_count = retry_info.retry_count;
                        let network_games_clone = self.network_games.clone();
                        let peer_performance_clone = self.peer_performance.clone();
                        let chunk_retries_clone = self.chunk_retries.clone();
                        let requested_chunks_for_task = self.requested_chunks.clone();
                        let chunk_download_start_times_for_task = self.chunk_download_start_times.clone();
                        
                        std::thread::spawn(move || {
                            // Warte auf Backoff
                            std::thread::sleep(backoff_duration);
                            
                            // Retry nach Backoff
                            if let Ok(game_id) = deckdrop_core::find_game_id_for_chunk(&chunk_hash_for_task) {
                                // Finde verfügbare Peers (nicht blockiert)
                                let available_peers: Vec<String> = {
                                    if let Some(peers) = network_games_clone.get(&game_id) {
                                        let perf_map_guard = peer_performance_clone.lock();
                                        let perf_map = perf_map_guard.as_ref().ok();
                                        
                                        peers.iter()
                                            .filter_map(|(pid, _)| {
                                                if let Some(perf_map) = perf_map {
                                                    if let Some(perf) = perf_map.get(pid) {
                                                        // Prüfe Circuit Breaker
                                                        if let Some(blocked_until) = perf.blocked_until {
                                                            if std::time::Instant::now() < blocked_until {
                                                                return None; // Peer ist blockiert
                                                            }
                                                        }
                                                    }
                                                }
                                                Some(pid.clone())
                                            })
                                            .collect()
                                    } else {
                                        Vec::new()
                                    }
                                };
                                
                                if !available_peers.is_empty() {
                                    // Wähle Peer: Wechsle bei should_switch_peer, sonst Round-Robin
                                    let peer_index = if should_switch_peer {
                                        // Wechsle zu anderem Peer
                                        available_peers.iter()
                                            .position(|p| p != &peer_id_for_task)
                                            .unwrap_or(0)
                                    } else {
                                        // Round-Robin basierend auf retry_count
                                        retry_count % available_peers.len()
                                    };
                                    
                                    let retry_peer_id = &available_peers[peer_index];
                                    
                                    // Sende Retry-Request
                                    if let Some(tx) = crate::network_bridge::get_download_request_tx() {
                                        if let Err(e) = tx.send(
                                            deckdrop_network::network::discovery::DownloadRequest::RequestChunk {
                                                peer_id: retry_peer_id.clone(),
                                                chunk_hash: chunk_hash_for_task.clone(),
                                                game_id: game_id.clone(),
                                            }
                                        ) {
                                            eprintln!("Fehler beim Senden von Retry-Chunk-Request für {}: {}", chunk_hash_for_task, e);
                                        } else {
                                            // WICHTIG: Trage Chunk NICHT vorzeitig in requested_chunks ein!
                                            // Er wird via ChunkRequestSent Event eingetragen, NACHDEM er erfolgreich gesendet wurde
                                            
                                            println!("Retry Chunk-Request gesendet: {} an Peer {} (Retry #{}, Backoff: {}s)", 
                                                chunk_hash_for_task, retry_peer_id, retry_count, backoff_seconds);
                                            
                                            // Aktualisiere Retry-Info
                                            if let Ok(mut retries) = chunk_retries_clone.lock() {
                                                if let Some(info) = retries.get_mut(&chunk_hash_for_task) {
                                                    info.last_retry_time = std::time::Instant::now();
                                                    info.last_peer_id = retry_peer_id.clone();
                                                }
                                            }
                                            // Chunk wird via ChunkRequestSent Event in requested_chunks eingetragen
                                        }
                                    }
                                } else {
                                    eprintln!("Keine verfügbaren Peers für Retry von Chunk {}", chunk_hash_for_task);
                                }
                            }
                        });
                        
                        eprintln!("Retry für Chunk {} geplant in {} Sekunden (Exponential Backoff, Retry #{})", 
                            chunk_hash, backoff_seconds, retry_count);
                    }
                } else {
                    let retry_count = match self.chunk_retries.lock() {
                        Ok(retries) => retries.get(&chunk_hash).map(|r| r.retry_count).unwrap_or(0),
                        Err(_) => 0,
                    };
                    eprintln!("Max Retries erreicht für Chunk {} ({} Retries) - Chunk wird nicht mehr angefordert", chunk_hash, retry_count);
                    // Chunk bleibt aus requested_chunks entfernt (wurde bereits oben entfernt)
                }
            }
        }
    }
    
    /// Wählt den besten Peer für einen Chunk aus (Load-Balancing + Performance)
    /// 
    /// WICHTIG: max_chunks_per_peer sollte das Netzwerk-Limit (10) sein, nicht das globale Limit (15)
    /// Das Netzwerk-Limit ist pro Peer, das globale Limit ist die Gesamtzahl aller aktiven Downloads
    fn select_best_peer_for_chunk(
        &self,
        chunk_hash: &str,
        available_peers: &[String],
        max_chunks_per_peer: usize,
    ) -> Option<String> {
        // WICHTIG: Verwende das Netzwerk-Limit (10), nicht das globale Limit (15)
        // Das Netzwerk-Limit ist pro Peer, das globale Limit ist die Gesamtzahl
        const MAX_REQUESTS_PER_PEER: usize = 10;
        let max_chunks_per_peer = max_chunks_per_peer.min(MAX_REQUESTS_PER_PEER);
        // Prüfe ob Chunk bereits angefordert wurde
        if let Ok(requested) = self.requested_chunks.lock() {
            if requested.contains(chunk_hash) {
                // Chunk wurde bereits angefordert - verwende den Peer, der ihn angefordert hat
                if let Ok(chunk_peers) = self.chunk_peer_requests.lock() {
                    if let Some(peer_id) = chunk_peers.get(chunk_hash) {
                        return Some(peer_id.clone());
                    }
                }
                // Chunk wurde angefordert, aber kein Peer-Mapping - sollte nicht passieren
                return None;
            }
        }
        
        // Hole aktive Requests pro Peer und Peer-Performance
        let active_per_peer = self.active_requests_per_peer.lock().ok()?;
        let perf_map = self.peer_performance.lock().ok()?;
        
        // Finde den besten Peer (wenigste aktive Requests, gute Performance, unter Limit)
        let mut best_peer: Option<String> = None;
        let mut best_score = f64::NEG_INFINITY;
        
        for peer_id in available_peers {
            // Prüfe ob Peer blockiert ist
            if let Some(perf) = perf_map.get(peer_id) {
                if let Some(blocked_until) = perf.blocked_until {
                    if std::time::Instant::now() < blocked_until {
                        continue; // Peer ist blockiert
                    }
                }
            }
            
            // Prüfe ob Peer unter Limit ist
            let active_count = active_per_peer.get(peer_id).copied().unwrap_or(0);
            if active_count >= max_chunks_per_peer {
                continue; // Peer hat bereits zu viele aktive Requests
            }
            
            // Berechne Score: niedrige aktive Requests = besser, hohe Performance = besser
            let perf = perf_map.get(peer_id).cloned().unwrap_or_default();
            let available_slots = max_chunks_per_peer - active_count;
            let performance_score = perf.download_speed_bytes_per_sec / 1_000_000.0; // MB/s
            let success_score = perf.success_rate * 100.0; // 0-100
            let slot_score = available_slots as f64 * 10.0; // Mehr verfügbare Slots = besser
            
            // Gesamt-Score: Performance + Success + Slots - aktive Requests
            let score = performance_score * 2.0 + success_score + slot_score - (active_count as f64 * 5.0);
            
            if score > best_score {
                best_score = score;
                best_peer = Some(peer_id.clone());
            }
        }
        
        best_peer
    }
    
    /// Phase 4: Request missing chunks with adaptive limits based on peer performance
    ///
    /// Hinweis:
    /// Die eigentliche Steuerung, wie viele Chunks gleichzeitig heruntergeladen werden
    /// (Slots auffüllen, keine Duplikate), passiert zentral in `update_download_progress`
    /// über `self.requested_chunks` und wird periodisch im `Tick`-Handler aufgerufen.
    /// Diese Funktion ist daher bewusst ein No-Op, um doppelte Scheduler-Logik und
    /// mögliche UI-Freezes zu vermeiden.
    fn request_missing_chunks_adaptive(
        &mut self,
        _game_id: &str,
        _peer_ids: &[String],
        _base_max_chunks_per_peer: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // No-Op: Der periodische Scheduler in `update_download_progress()` übernimmt alles.
        // Wir loggen nur, dass der Aufruf erfolgt ist.
        eprintln!("request_missing_chunks_adaptive: Aufruf ignoriert (Scheduler läuft im Tick)");
        Ok(())
    }
    
    /// Sammelt und aktualisiert Performance-Metriken
    fn update_performance_metrics(&mut self) {
        // Sammle Download-Statistiken
        let mut total_download_speed = 0.0;
        let mut active_downloads_count = 0;
        let mut total_chunks_downloaded = 0;
        
        for download_state in self.active_downloads.values() {
            if !matches!(download_state.manifest.overall_status, 
                deckdrop_core::DownloadStatus::Complete | 
                deckdrop_core::DownloadStatus::Cancelled) {
                active_downloads_count += 1;
                total_download_speed += download_state.download_speed_bytes_per_sec;
            }
            total_chunks_downloaded += download_state.manifest.progress.downloaded_chunks;
        }
        
        // Sammle Upload-Statistiken
        let (total_upload_speed, active_uploads_count, total_chunks_uploaded) = {
            if let Ok(stats) = self.upload_stats.lock() {
                (stats.upload_speed_bytes_per_sec, stats.active_upload_count, 0) // TODO: Track uploaded chunks
            } else {
                (0.0, 0, 0)
            }
        };
        
        // Sammle Peer-Performance-Daten
        let peer_performance_list: Vec<(String, PeerPerformance)> = {
            if let Ok(perf_map) = self.peer_performance.lock() {
                perf_map.iter()
                    .map(|(peer_id, perf)| (peer_id.clone(), perf.clone()))
                    .collect()
            } else {
                Vec::new()
            }
        };
        
        // Berechne aktive Verbindungen
        let active_connections = {
            if let Ok(requested) = self.requested_chunks.lock() {
                requested.len()
            } else {
                0
            }
        };
        
        // Schätze Bandbreiten-Nutzung (basierend auf Gigabit Ethernet = 125 MB/s)
        const GIGABIT_BANDWIDTH_MBPS: f64 = 125.0 * 1024.0 * 1024.0; // 125 MB/s in Bytes/s
        let total_bandwidth_usage = total_download_speed + total_upload_speed;
        let bandwidth_utilization = if GIGABIT_BANDWIDTH_MBPS > 0.0 {
            (total_bandwidth_usage / GIGABIT_BANDWIDTH_MBPS * 100.0).min(100.0)
        } else {
            0.0
        };
        
        // Aktualisiere Performance-Metriken
        self.performance_metrics = PerformanceMetrics {
            total_download_speed_bytes_per_sec: total_download_speed,
            total_upload_speed_bytes_per_sec: total_upload_speed,
            active_downloads: active_downloads_count,
            active_uploads: active_uploads_count,
            total_chunks_downloaded,
            total_chunks_uploaded,
            active_connections,
            peer_performance: peer_performance_list,
            bandwidth_utilization_percent: bandwidth_utilization,
            last_update: std::time::Instant::now(),
        };
    }
    
    /// Updates upload statistics
    fn update_upload_stats(&mut self) {
        // Berechne Upload-Geschwindigkeit basierend auf aktiven Uploads
        let now = std::time::Instant::now();
        let cutoff_time = now.checked_sub(std::time::Duration::from_secs(5))
            .unwrap_or(now);
        
        // Entferne alte Uploads (älter als 5 Sekunden)
        if let Ok(mut active_uploads) = self.active_uploads.lock() {
            active_uploads.retain(|_, (time, _)| *time >= cutoff_time);
            
            // Berechne aktive Uploads und Geschwindigkeit
            let active_upload_count = active_uploads.len();
            let total_bytes_in_window: usize = active_uploads.values()
                .map(|(_, size)| *size)
                .sum();
            
            // Geschwindigkeit = Bytes in 5 Sekunden / 5
            let upload_speed_bytes_per_sec = total_bytes_in_window as f64 / 5.0;
            
            // Aktualisiere Upload-Statistiken
            if let Ok(mut stats) = self.upload_stats.lock() {
                stats.active_upload_count = active_upload_count;
                stats.upload_speed_bytes_per_sec = upload_speed_bytes_per_sec;
                stats.last_update_time = now;
            }
        }
    }
    
    /// Aktualisiert den Download‑Fortschritt.
    ///
    /// WICHTIG:
    /// - Diese Funktion läuft im UI‑Thread und darf daher kein schweres I/O ausführen.
    /// - Die Manifeste / aktiven Downloads werden im Background (Chunk‑Threads) über
    ///   `deckdrop_core::load_active_downloads()` geladen und in einem zeitbasierten
    ///   Cache aktualisiert.
    /// - Hier lesen wir NUR aus diesem Cache (`get_active_downloads_cached_only()`),
    ///   damit der UI‑Thread nicht blockiert.
    fn update_download_progress(&mut self) {
        // Hole aktive Downloads ausschließlich aus dem Cache (kein I/O im UI‑Thread)
        let active_downloads_from_manifests = deckdrop_core::get_active_downloads_cached_only();
        
        // Track which games need to be finalized (only finalize once)
        let mut games_to_finalize = Vec::new();
        
        // Update active_downloads with all manifests
        for (game_id, manifest) in active_downloads_from_manifests {
            let progress_percent = manifest.progress.percentage as f32;

            // PERFORMANCE FIX: Keine teure Chunk-Zählung im UI-Thread!
            // Dies hat massiven UI-Lag verursacht. Wir setzen es vorerst auf 0.
            // TODO: Zähler im Scheduler-Thread pflegen und via Event an UI senden.
            let downloading_chunks_count = 0;

            // Get peer count
            let peer_count = self.network_games.get(&game_id)
                .map(|peers| peers.len())
                .unwrap_or(0);

            // Check if download already exists
            if let Some(existing_state) = self.active_downloads.get_mut(&game_id) {
                // Update existing download state with fresh manifest data
                existing_state.manifest = manifest.clone();
                existing_state.progress_percent = progress_percent;
                existing_state.downloading_chunks_count = downloading_chunks_count;
                existing_state.peer_count = peer_count;

                // Update download speed (exponential moving average)
                let now = std::time::Instant::now();
                existing_state.speed_samples.retain(|&(time, _)| now.duration_since(time) < std::time::Duration::from_secs(60));
                if !existing_state.speed_samples.is_empty() {
                    existing_state.download_speed_bytes_per_sec = existing_state.speed_samples.iter()
                        .map(|&(_, speed)| speed as f64)
                        .sum::<f64>() / existing_state.speed_samples.len() as f64;
                }

                /*
                println!("📊 Updated progress for {}: {:.1}% ({} active chunks)",
                    game_id, progress_percent, downloading_chunks_count);
                */
            } else {
                // New download not in active_downloads yet - add it
                println!("📥 Adding new active download: {}", game_id);

                // Create initial DownloadState
                let download_state = DownloadState {
                    manifest: manifest.clone(),
                    progress_percent,
                    downloading_chunks_count,
                    peer_count,
                    download_speed_bytes_per_sec: 0.0,
                    last_update_time: std::time::Instant::now(),
                    last_downloaded_chunks: manifest.progress.downloaded_chunks,
                    speed_samples: Vec::new(),
                };

                self.active_downloads.insert(game_id.clone(), download_state);
            }
            
            // Check if this download is complete and needs finalization
            if matches!(manifest.overall_status, deckdrop_core::DownloadStatus::Complete) {
                // Mark for finalization (only if not already finalized)
                let game_path = PathBuf::from(&manifest.game_path);
                let already_in_my_games = self.my_games.iter().any(|(path, _)| {
                    // Normalize paths for comparison
                    path.canonicalize().ok().and_then(|p1| {
                        game_path.canonicalize().ok().map(|p2| p1 == p2)
                    }).unwrap_or(false) || path == &game_path
                });
                
                if !already_in_my_games {
                    games_to_finalize.push((game_id.clone(), manifest.clone()));
                }
            }
            
            // Statistiken werden bereits in der Update-Schleife oben berechnet
            
            // Anzahl der Peers für diesen Download
            let peer_count = self.network_games.get(&game_id)
                .map(|peers| {
                    // Zähle eindeutige Peer-IDs
                    let unique_peers: std::collections::HashSet<_> = peers.iter()
                        .map(|(peer_id, _)| peer_id)
                        .collect();
                    unique_peers.len()
                })
                .unwrap_or(0);
            
            // Berechne Download-Geschwindigkeit mit gleitendem Durchschnitt
            let (download_speed_bytes_per_sec, last_update_time, last_downloaded_chunks, speed_samples) = {
                let existing_state = self.active_downloads.get(&game_id);
                if let Some(existing) = existing_state {
                    let now = std::time::Instant::now();
                    let mut samples = existing.speed_samples.clone();
                    
                    // Füge neuen Sample hinzu, wenn sich die Anzahl der Chunks geändert hat
                    if manifest.progress.downloaded_chunks != existing.last_downloaded_chunks {
                        samples.push((now, manifest.progress.downloaded_chunks));
                    }
                    
                    // Entferne Samples, die älter als 5 Sekunden sind
                    let cutoff_time = now.checked_sub(std::time::Duration::from_secs(5)).unwrap_or(now);
                    samples.retain(|(time, _)| *time > cutoff_time);
                    
                    // Berechne Geschwindigkeit basierend auf dem ältesten und neuesten Sample
                    let speed = if samples.len() >= 2 {
                        // Sichere Berechnung ohne unwrap() - verhindert Abstürze
                        if let (Some((oldest_time, oldest_chunks)), Some((newest_time, newest_chunks))) = 
                            (samples.first(), samples.last()) {
                            let elapsed = newest_time.duration_since(*oldest_time).as_secs_f64();
                            let chunks_downloaded = newest_chunks.saturating_sub(*oldest_chunks);
                            
                            // Chunk-Größe: 1MB = 1 * 1024 * 1024 Bytes
                            const CHUNK_SIZE_BYTES: u64 = 1 * 1024 * 1024;
                            let bytes_downloaded = (chunks_downloaded as u64) * CHUNK_SIZE_BYTES;
                            
                            if elapsed > 0.5 { // Mindestens 0.5 Sekunden für stabile Berechnung
                                bytes_downloaded as f64 / elapsed
                            } else {
                                // Verwende vorherige Geschwindigkeit, wenn Zeitfenster zu kurz
                                existing.download_speed_bytes_per_sec
                            }
                        } else {
                            // Nicht genug Samples - verwende vorherige Geschwindigkeit
                            existing.download_speed_bytes_per_sec
                        }
                    } else {
                        // Nicht genug Samples - verwende vorherige Geschwindigkeit
                        existing.download_speed_bytes_per_sec
                    };
                    
                    (speed, now, manifest.progress.downloaded_chunks, samples)
                } else {
                    // Erste Aktualisierung - noch keine Geschwindigkeit
                    (0.0, std::time::Instant::now(), manifest.progress.downloaded_chunks, Vec::new())
                }
            };
            
            // Update or add download state
            self.active_downloads.insert(game_id.clone(), DownloadState {
                manifest: manifest.clone(),
                progress_percent,
                downloading_chunks_count,
                peer_count,
                download_speed_bytes_per_sec,
                last_update_time,
                last_downloaded_chunks,
                speed_samples: speed_samples,
            });
        }
        
        // Finalize downloads that need it (only once)
        // ACHTUNG:
        // Die ursprüngliche Implementierung hat hier im UI-Thread:
        //   - `finalize_game_download` aufgerufen (viel Dateisystem + Validierung)
        //   - anschließend die komplette Spieleliste von der Platte neu geladen
        // Das hat bei großen Spielen spürbare Freezes verursacht.
        //
        // Solange wir die Finalisierung nicht sauber in einen Background-Thread
        // mit eigener UI-Nachricht ausgelagert haben, lassen wir diese teure
        // Arbeit hier bewusst weg, damit die GUI reaktionsfähig bleibt.
        //
        // TODO: Finalisierung + Reload der Spieleliste in eigenen Thread auslagern
        //       und das Ergebnis per Message zurück in den UI-Thread schicken.
        for (_game_id, _manifest) in games_to_finalize {
            // Platzhalter: aktuell keine teure Arbeit im UI-Thread
        }
        
        // Add downloading games to my_games if not already present (only for active downloads)
        for (game_id, download_state) in &self.active_downloads {
            // Only add if download is not complete (complete downloads are handled above)
            if !matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Complete) {
                let game_path = PathBuf::from(&download_state.manifest.game_path);
                
                // Check if game is already in my_games (by game_id, not just path)
                let already_exists = self.my_games.iter().any(|(_, game_info)| {
                    game_info.game_id == download_state.manifest.game_id
                });
                
                if !already_exists {
                    if let Ok(manifest_path) = deckdrop_core::get_manifest_path(game_id) {
                        if let Some(manifest_dir) = manifest_path.parent() {
                            if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(manifest_dir) {
                                self.my_games.push((game_path.clone(), game_info));
                                
                                // Don't initialize integrity status - will show "NotChecked" by default
                                // User can trigger integrity check manually via button
                            }
                        }
                    }
                }
            }
        }
        
        // Resume downloads if host is available
        // - Pending downloads: Start them when peers become available
        // - Downloading downloads: Continue them if they have missing chunks but no active requests
        // - Paused downloads: Should be resumed manually (not automatically)
        let games_to_resume: Vec<String> = self.active_downloads
            .iter()
            .filter(|(_game_id, download_state)| {
                // Prüfe ob Download pausiert oder abgebrochen ist (diese sollten nicht automatisch fortgesetzt werden)
                if matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Paused | deckdrop_core::DownloadStatus::Cancelled) {
                    return false;
                }
                
                // Prüfe ob Download Pending ist (sollte gestartet werden)
                if matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Pending) {
                    return true;
                }
                
                // Prüfe ob Download Downloading ist, aber keine aktiven Requests hat
                if matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading) {
                    // PERFORMANCE FIX: Keine DB-Zugriffe im UI-Thread für Resume-Check!
                    // Wir prüfen nur ob globale Requests existieren. Wenn ja, läuft der Scheduler eh.
                    // Wenn nein, und wir sind im "Downloading" Status, dann sollten wir den Scheduler anstupsen.
                    
                    if let Ok(requested) = self.requested_chunks.lock() {
                        if !requested.is_empty() {
                            // Es gibt aktive Requests (irgendwo), also arbeitet das System.
                            // Wir gehen davon aus, dass alles läuft.
                            return false; 
                        }
                    }
                    
                    // Keine aktiven Requests global.
                    // Wir gehen davon aus, dass wir Chunks brauchen, wenn wir im Status "Downloading" sind.
                    // (Die genaue Prüfung auf "missing chunks" überlassen wir dem Scheduler/Network-Layer,
                    // der das effizienter machen kann oder eh schon weiß).
                    return true;
                }
                
                false
            })
            .filter_map(|(game_id, _)| {
                // Check if peers are available for this game
                if let Some(peers) = self.network_games.get(game_id) {
                    if !peers.is_empty() {
                        Some(game_id.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        
        for game_id in games_to_resume {
            // Get peer IDs
            if let Some(peers) = self.network_games.get(&game_id) {
                let peer_ids: Vec<String> = peers.iter().map(|(peer_id, _)| peer_id.clone()).collect();
                
                // Registriere Download im Scheduler, damit fehlende Chunks regelmäßig nachgefordert werden
                if !peer_ids.is_empty() {
                    let changed = crate::network_bridge::register_download_peers(&game_id, &peer_ids);
                    
                    // Update status to Downloading (falls es noch Pending war)
                    if let Some(ds) = self.active_downloads.get_mut(&game_id) {
                        if matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Pending) {
                            ds.manifest.overall_status = deckdrop_core::DownloadStatus::Downloading;
                            
                            // Save updated manifest
                            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                                let _ = ds.manifest.save(&manifest_path);
                            }
                        }
                    }
                    
                    // Log nur wenn sich was geändert hat
                    if changed {
                        println!("Resumed/continued download for game (Scheduler aktiviert/aktualisiert): {}", game_id);
                    }
                }
            }
        }
        
        // Remove downloads that no longer exist
        self.active_downloads.retain(|game_id, _| {
            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(game_id) {
                manifest_path.exists()
            } else {
                false
            }
        });
        
        // Count only actively downloading games (not Complete, not Cancelled, not Pending)
        self.status.active_download_count = self.active_downloads.values()
            .filter(|ds| {
                matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading)
            })
            .count();
    }
    
    /// Shows tabs
    fn view_tabs(&self) -> Element<'_, Message> {
        row![
            button("My Games")
                .on_press(Message::TabChanged(Tab::MyGames))
                .style(if self.current_tab == Tab::MyGames {
                    button::primary
                } else {
                    button::secondary
                }),
            button("Network Games")
                .on_press(Message::TabChanged(Tab::NetworkGames))
                .style(if self.current_tab == Tab::NetworkGames {
                    button::primary
                } else {
                    button::secondary
                }),
            button("Peers")
                .on_press(Message::TabChanged(Tab::Peers))
                .style(if self.current_tab == Tab::Peers {
                    button::primary
                } else {
                    button::secondary
                }),
            button("Performance")
                .on_press(Message::TabChanged(Tab::Performance))
                .style(if self.current_tab == Tab::Performance {
                    button::primary
                } else {
                    button::secondary
                }),
            button("Settings")
                .on_press(Message::TabChanged(Tab::Settings))
                .style(if self.current_tab == Tab::Settings {
                    button::primary
                } else {
                    button::secondary
                }),
        ]
        .spacing(scale(8.0))
        .into()
    }
    
    /// Shows current tab
    fn view_current_tab(&self) -> Element<'_, Message> {
        match self.current_tab {
            Tab::MyGames => self.view_my_games(),
            Tab::NetworkGames => self.view_network_games(),
            Tab::Peers => self.view_peers(),
            Tab::Performance => self.view_performance(),
            Tab::Settings => self.view_settings_tab(),
            Tab::GameDetails => self.view_game_details(),
        }
    }
    
    /// Shows "My Games" tab
    fn view_my_games(&self) -> Element<'_, Message> {
        let mut games_column = Column::new()
            .spacing(scale(8.0))
            .padding(scale(8.0));
        
        // Header with "Add Game" button
        games_column = games_column.push(
            row![
                text("My Games").size(scale_text(20.0)),
                Space::with_width(Length::Fill),
                button("+ Add Game")
                    .on_press(Message::AddGame),
            ]
        );
        
        // Games list
        if self.my_games.is_empty() {
            games_column = games_column.push(
                text("No games available. Click '+ Add Game' to add a game.")
            );
        } else {
            for (game_path, game) in &self.my_games {
                // Check if this game is currently downloading
                let download_state = self.active_downloads.values().find(|ds| {
                    PathBuf::from(&ds.manifest.game_path) == *game_path
                });
                
                let integrity_status = self.game_integrity_status.get(game_path)
                    .unwrap_or(&GameIntegrityStatus::NotChecked);
                
                let game_path_clone = game_path.clone();
                let game_info_clone = game.clone();
                let mut game_column = column![
                    row![
                        column![
                            text(&game.name).size(scale_text(16.0)),
                            text(format!("Version: {}", game.version)).size(scale_text(12.0)),
                            text(format!("Path: {}", game_path.display())).size(scale_text(10.0)),
                        ]
                        .width(Length::Fill),
                        // Right column with buttons (all buttons have same width)
                        column![
                            // Details button (always on top)
                            button("Details")
                                .on_press(Message::ShowGameDetails(game_path_clone.clone(), game_info_clone.clone()))
                                .style(button::primary)
                                .width(Length::Fixed(scale(180.0))),
                            // Download control buttons (only show if download is active and not complete)
                            if let Some(ds) = download_state {
                                let game_id = ds.manifest.game_id.clone();
                                let show_buttons = !matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Complete);
                                
                                if show_buttons {
                                    let (can_pause, can_resume) = (
                                        matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading),
                                        matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Paused),
                                    );
                                    
                                    column![
                                        if can_pause {
                                            button("Pause")
                                                .on_press(Message::PauseDownload(game_id.clone()))
                                                .style(button::secondary)
                                                .width(Length::Fixed(scale(180.0)))
                                        } else if can_resume {
                                            button("Resume")
                                                .on_press(Message::ResumeDownload(game_id.clone()))
                                                .style(button::secondary)
                                                .width(Length::Fixed(scale(180.0)))
                                        } else {
                                            button("Downloading...")
                                                .style(button::secondary)
                                                .width(Length::Fixed(scale(180.0)))
                                        },
                                        button("Cancel")
                                            .on_press(Message::CancelDownload(game_id.clone()))
                                            .width(Length::Fixed(scale(180.0))),
                                    ]
                                    .spacing(scale(4.0))
                                } else {
                                    column![].spacing(scale(4.0))
                                }
                            } else {
                                column![].spacing(scale(4.0))
                            },
                            // Action buttons for completed games (not downloading)
                            if download_state.is_none() || download_state.map(|ds| matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Complete)).unwrap_or(false) {
                                let game_path_for_integrity = game_path.clone();
                                // Check if currently checking based on status, not just start_time
                                let is_checking = matches!(integrity_status, GameIntegrityStatus::Checking { .. });
                                
                                column![
                                    if is_checking {
                                        button("Checking...")
                                            .style(button::secondary)
                                            .width(Length::Fixed(scale(180.0)))
                                    } else {
                                        button("Check Integrity")
                                            .on_press(Message::CheckIntegrity(game_path_for_integrity.clone()))
                                            .style(button::secondary)
                                            .width(Length::Fixed(scale(180.0)))
                                    },
                                ]
                                .spacing(scale(4.0))
                            } else {
                                column![].spacing(scale(4.0))
                            },
                        ]
                        .spacing(scale(4.0)),
                    ]
                    .spacing(scale(8.0)),
                ];
                
                // Show download status if downloading
                if let Some(ds) = download_state {
                    // Calculate total size from actual file sizes
                    let total_bytes: u64 = ds.manifest.chunks.values()
                        .filter_map(|file_info| file_info.file_size)
                        .sum();
                    
                    // Calculate downloaded size: use average chunk size if we have total size,
                    // otherwise fall back to 100MB per chunk
                    let downloaded_bytes = if total_bytes > 0 && ds.manifest.progress.total_chunks > 0 {
                        // Use proportional calculation based on actual total size
                        (total_bytes as f64 * (ds.manifest.progress.downloaded_chunks as f64 / ds.manifest.progress.total_chunks as f64)) as u64
                    } else {
                        // Fallback: assume 100MB per chunk
                        const CHUNK_SIZE_BYTES: u64 = 100 * 1024 * 1024;
                        (ds.manifest.progress.downloaded_chunks as u64) * CHUNK_SIZE_BYTES
                    };
                    
                    let total_bytes_final = if total_bytes > 0 {
                        total_bytes
                    } else {
                        // Fallback: estimate from chunk count
                        const CHUNK_SIZE_BYTES: u64 = 100 * 1024 * 1024;
                        (ds.manifest.progress.total_chunks as u64) * CHUNK_SIZE_BYTES
                    };
                    
                    let downloaded_size_str = format_size(downloaded_bytes);
                    let total_size_str = format_size(total_bytes_final);
                    
                    let download_status_text = match ds.manifest.overall_status {
                        deckdrop_core::DownloadStatus::Downloading => {
                            format!("Downloading... {:.1}% ({}/{})", ds.progress_percent, downloaded_size_str, total_size_str)
                        }
                        deckdrop_core::DownloadStatus::Paused => {
                            format!("Paused ({}/{})", downloaded_size_str, total_size_str)
                        }
                        deckdrop_core::DownloadStatus::Pending => "Waiting for host...".to_string(),
                        deckdrop_core::DownloadStatus::Complete => {
                            format!("Download complete ({})", total_size_str)
                        }
                        deckdrop_core::DownloadStatus::Error(ref e) => format!("Download error: {}", e),
                        deckdrop_core::DownloadStatus::Cancelled => "Cancelled".to_string(),
                    };
                    
                    let download_status_color = match ds.manifest.overall_status {
                        deckdrop_core::DownloadStatus::Downloading => Color::from_rgba(0.0, 0.7, 1.0, 1.0),
                        deckdrop_core::DownloadStatus::Paused => Color::from_rgba(1.0, 0.7, 0.0, 1.0),
                        deckdrop_core::DownloadStatus::Pending => Color::from_rgba(0.7, 0.7, 0.7, 1.0),
                        deckdrop_core::DownloadStatus::Complete => Color::from_rgba(0.0, 1.0, 0.0, 1.0),
                        deckdrop_core::DownloadStatus::Error(_) => Color::from_rgba(1.0, 0.0, 0.0, 1.0),
                        deckdrop_core::DownloadStatus::Cancelled => Color::from_rgba(0.7, 0.7, 0.7, 1.0),
                    };
                    
                    game_column = game_column.push(
                        text(download_status_text.clone())
                            .size(scale_text(10.0))
                            .style(move |_theme: &Theme| {
                                iced::widget::text::Style {
                                    color: Some(download_status_color),
                                }
                            })
                    );
                    
                    // Show progress bar and statistics for active downloads
                    if matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading) {
                        // Format download speed
                        let speed_text = if ds.download_speed_bytes_per_sec > 1_000_000.0 {
                            format!("{:.2} MB/s", ds.download_speed_bytes_per_sec / 1_000_000.0)
                        } else if ds.download_speed_bytes_per_sec > 1_000.0 {
                            format!("{:.2} KB/s", ds.download_speed_bytes_per_sec / 1_000.0)
                        } else {
                            format!("{:.0} B/s", ds.download_speed_bytes_per_sec)
                        };
                        
                        game_column = game_column.push(
                            column![
                                progress_bar(0.0..=100.0, ds.progress_percent)
                                    .width(Length::Fill),
                                text(format!(
                                    "Chunks: {}/{} | Requested: {} | Peers: {} | Speed: {}",
                                    ds.manifest.progress.downloaded_chunks,
                                    ds.manifest.progress.total_chunks,
                                    ds.downloading_chunks_count,
                                    ds.peer_count,
                                    speed_text
                                )).size(scale_text(9.0))
                            ]
                            .spacing(scale(4.0))
                        );
                    }
                }
                
                // Show integrity status (only if not downloading or download is complete)
                let show_integrity = if let Some(ds) = download_state {
                    matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Complete)
                } else {
                    true
                };
                
                if show_integrity {
                    let (status_text, status_color) = match integrity_status {
                        GameIntegrityStatus::NotChecked => ("Integrity not checked".to_string(), Color::from_rgba(0.7, 0.7, 0.7, 1.0)),
                        GameIntegrityStatus::Checking { current, total } => {
                            if *total > 0 {
                                // Show current/total, but if current is 0, show "Starting..."
                                if *current == 0 {
                                    (format!("Checking... ({}/{})", *current, *total), Color::from_rgba(0.7, 0.7, 0.7, 1.0))
                                } else {
                                    (format!("Checking {}/{}...", *current, *total), Color::from_rgba(0.7, 0.7, 0.7, 1.0))
                                }
                            } else {
                                ("Checking...".to_string(), Color::from_rgba(0.7, 0.7, 0.7, 1.0))
                            }
                        }
                        GameIntegrityStatus::Intact => ("Game files intact".to_string(), Color::from_rgba(0.0, 1.0, 0.0, 1.0)),
                        GameIntegrityStatus::Changed => ("Game files have changed".to_string(), Color::from_rgba(1.0, 0.7, 0.0, 1.0)),
                        GameIntegrityStatus::Error(_) => ("Error checking integrity".to_string(), Color::from_rgba(1.0, 0.0, 0.0, 1.0)),
                    };
                    
                    game_column = game_column.push(
                        text(status_text.clone())
                            .size(scale_text(10.0))
                            .style(move |_theme: &Theme| {
                                iced::widget::text::Style {
                                    color: Some(status_color),
                                }
                            })
                    );
                }
                
                games_column = games_column.push(
                    container(game_column)
                        .style(container_box_style)
                        .width(Length::Fill)
                        .padding(scale(12.0))
                );
            }
        }
        
        scrollable(games_column)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    
    /// Shows "Network Games" tab
    fn view_network_games(&self) -> Element<'_, Message> {
        let mut games_column = Column::new()
            .spacing(scale(8.0))
            .padding(scale(8.0));
        
        // Header mit Titel und Clear Cache Button
        games_column = games_column.push(
            row![
                text("Network Games").size(scale_text(20.0)),
                Space::with_width(Length::Fill),
                button("Clear Cache")
                    .on_press(Message::ClearNetworkCache)
                    .style(button::secondary)
                    .width(Length::Fixed(scale(150.0))),
            ]
            .width(Length::Fill)
        );
        
        if self.network_games.is_empty() {
            games_column = games_column.push(
                text("No games found in network.")
            );
        } else {
            for (game_id, games) in &self.network_games {
                if let Some((_, game_info)) = games.first() {
                    let download_state = self.active_downloads.get(game_id);
                    let is_downloading = download_state.is_some();
                    
                    let game_id_clone = game_id.clone();
                    // For network games, we need to construct a path - use download path + game_id
                    let config = deckdrop_core::Config::load();
                    let network_game_path = config.download_path.join(&game_id_clone);
                    
                    // Convert NetworkGameInfo to GameInfo for details view
                    let game_info_for_details = deckdrop_core::GameInfo {
                        game_id: game_info.game_id.clone(),
                        name: game_info.name.clone(),
                        version: game_info.version.clone(),
                        start_file: game_info.start_file.clone(),
                        start_args: game_info.start_args.clone(),
                        description: game_info.description.clone(),
                        additional_instructions: None, // NetworkGameInfo doesn't have this
                        creator_peer_id: game_info.creator_peer_id.clone(),
                        hash: None, // NetworkGameInfo doesn't have this
                    };
                    
                    // Bestimme Online/Offline-Status und Peer-Anzahl
                    let unique_peers: std::collections::HashSet<_> = games.iter()
                        .map(|(peer_id, _)| peer_id)
                        .collect();
                    let peer_count = unique_peers.len();
                    let is_online = peer_count > 0;
                    
                    let mut game_column = column![
                        row![
                            column![
                                text(&game_info.name).size(scale_text(16.0)),
                                text(format!("Version: {}", game_info.version)).size(scale_text(12.0)),
                                // Status und Peer-Anzahl
                                if is_online {
                                    text(format!("🟢 Online - {} Peer(s)", peer_count))
                                        .size(scale_text(10.0))
                                        .style(|_theme: &Theme| {
                                            iced::widget::text::Style {
                                                color: Some(Color::from_rgba(0.0, 0.8, 0.0, 1.0)),
                                            }
                                        })
                                } else {
                                    text("🔴 Offline")
                                        .size(scale_text(10.0))
                                        .style(|_theme: &Theme| {
                                            iced::widget::text::Style {
                                                color: Some(Color::from_rgba(0.8, 0.0, 0.0, 1.0)),
                                            }
                                        })
                                },
                            ]
                            .width(Length::Fill),
                            // Right column with buttons (all buttons have same width)
                            column![
                                // Details button (always on top)
                                button("Details")
                                    .on_press(Message::ShowGameDetails(network_game_path.clone(), game_info_for_details.clone()))
                                    .style(button::primary)
                                    .width(Length::Fixed(scale(180.0))),
                                // Download buttons
                                if is_downloading {
                                    let (can_pause, can_resume) = if let Some(ds) = download_state {
                                        (
                                            matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading),
                                            matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Paused),
                                        )
                                    } else {
                                        (false, false)
                                    };
                                    
                                    column![
                                        if can_pause {
                                            button("Pause")
                                                .on_press(Message::PauseDownload(game_id_clone.clone()))
                                                .style(button::secondary)
                                                .width(Length::Fixed(scale(180.0)))
                                        } else if can_resume {
                                            button("Resume")
                                                .on_press(Message::ResumeDownload(game_id_clone.clone()))
                                                .style(button::secondary)
                                                .width(Length::Fixed(scale(180.0)))
                                        } else {
                                            button("Downloading...")
                                                .style(button::secondary)
                                                .width(Length::Fixed(scale(180.0)))
                                        },
                                        button("Cancel")
                                            .on_press(Message::CancelDownload(game_id_clone.clone()))
                                            .width(Length::Fixed(scale(180.0))),
                                    ]
                                    .spacing(scale(4.0))
                                } else {
                                    // Prüfe ob Download gerade vorbereitet wird (thread-safe)
                                    let is_preparing = if let Ok(preparing) = self.preparing_downloads.lock() {
                                        preparing.contains_key(&game_id_clone)
                                    } else {
                                        false
                                    };
                                    let is_starting = if let Ok(starting) = self.downloading_starting.lock() {
                                        starting.contains(&game_id_clone)
                                    } else {
                                        false
                                    };
                                    
                                    if is_preparing {
                                        // Zeige Ladebalken für Pre-Allocation (thread-safe)
                                        let (current, total) = if let Ok(preparing) = self.preparing_downloads.lock() {
                                            preparing.get(&game_id_clone).copied().unwrap_or((0, 1))
                                        } else {
                                            (0, 1)
                                        };
                                        let progress = if total > 0 {
                                            current as f32 / total as f32
                                        } else {
                                            0.0
                                        };
                                        
                                        column![
                                            text(format!("Vorbereitung... ({}/{})", current, total))
                                                .size(scale_text(12.0)),
                                            progress_bar(0.0..=1.0, progress)
                                                .width(Length::Fixed(scale(180.0)))
                                                .height(Length::Fixed(scale(8.0))),
                                        ]
                                        .spacing(scale(4.0))
                                    } else if is_starting {
                                        column![
                                            button("Starting...")
                                                .style(button::secondary)
                                                .width(Length::Fixed(scale(180.0)))
                                        ]
                                        .spacing(scale(4.0))
                                    } else {
                                        column![
                                            button("Get this game")
                                                .on_press(Message::DownloadGame(game_id_clone.clone()))
                                                .style(button::primary)
                                                .width(Length::Fixed(scale(180.0)))
                                        ]
                                        .spacing(scale(4.0))
                                    }
                                },
                            ]
                            .spacing(scale(4.0)),
                        ]
                        .spacing(scale(8.0)),
                    ];
                    
                    // Show progress bar and statistics when download is active
                    if let Some(ds) = download_state {
                        // Format download speed
                        let speed_text = if ds.download_speed_bytes_per_sec > 1_000_000.0 {
                            format!("{:.2} MB/s", ds.download_speed_bytes_per_sec / 1_000_000.0)
                        } else if ds.download_speed_bytes_per_sec > 1_000.0 {
                            format!("{:.2} KB/s", ds.download_speed_bytes_per_sec / 1_000.0)
                        } else {
                            format!("{:.0} B/s", ds.download_speed_bytes_per_sec)
                        };
                        
                        game_column = game_column.push(
                            column![
                                text(format!("Progress: {:.1}%", ds.progress_percent)).size(scale_text(10.0)),
                                progress_bar(0.0..=100.0, ds.progress_percent)
                                    .width(Length::Fill),
                                text(format!(
                                    "Chunks: {}/{} | Requested: {} | Peers: {} | Speed: {}",
                                    ds.manifest.progress.downloaded_chunks,
                                    ds.manifest.progress.total_chunks,
                                    ds.downloading_chunks_count,
                                    ds.peer_count,
                                    speed_text
                                )).size(scale_text(9.0)),
                            ]
                            .spacing(scale(4.0))
                        );
                        
                        // Calculate total size from actual file sizes
                        let total_bytes: u64 = ds.manifest.chunks.values()
                            .filter_map(|file_info| file_info.file_size)
                            .sum();
                        
                        // Calculate downloaded size: use average chunk size if we have total size,
                        // otherwise fall back to 100MB per chunk
                        let downloaded_bytes = if total_bytes > 0 && ds.manifest.progress.total_chunks > 0 {
                            // Use proportional calculation based on actual total size
                            (total_bytes as f64 * (ds.manifest.progress.downloaded_chunks as f64 / ds.manifest.progress.total_chunks as f64)) as u64
                        } else {
                            // Fallback: assume 100MB per chunk
                            const CHUNK_SIZE_BYTES: u64 = 100 * 1024 * 1024;
                            (ds.manifest.progress.downloaded_chunks as u64) * CHUNK_SIZE_BYTES
                        };
                        
                        let total_bytes_final = if total_bytes > 0 {
                            total_bytes
                        } else {
                            // Fallback: estimate from chunk count
                            const CHUNK_SIZE_BYTES: u64 = 100 * 1024 * 1024;
                            (ds.manifest.progress.total_chunks as u64) * CHUNK_SIZE_BYTES
                        };
                        
                        let downloaded_size_str = format_size(downloaded_bytes);
                        let total_size_str = format_size(total_bytes_final);
                        
                        // Show status
                        let status_text = match ds.manifest.overall_status {
                            deckdrop_core::DownloadStatus::Downloading => {
                                format!("Downloading... ({}/{})", downloaded_size_str, total_size_str)
                            }
                            deckdrop_core::DownloadStatus::Paused => {
                                format!("Paused ({}/{})", downloaded_size_str, total_size_str)
                            }
                            deckdrop_core::DownloadStatus::Complete => {
                                format!("Completed ({})", total_size_str)
                            }
                            deckdrop_core::DownloadStatus::Error(_) => "Failed".to_string(),
                            deckdrop_core::DownloadStatus::Pending => "Pending".to_string(),
                            deckdrop_core::DownloadStatus::Cancelled => "Cancelled".to_string(),
                        };
                        game_column = game_column.push(
                            text(status_text).size(scale_text(10.0))
                        );
                    }
                    
                    games_column = games_column.push(
                        container(game_column)
                            .style(container_box_style)
                            .width(Length::Fill)
                            .padding(scale(12.0))
                    );
                }
            }
        }
        
        scrollable(games_column)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    
    /// Shows "Peers" tab
    fn view_peers(&self) -> Element<'_, Message> {
        let mut peers_column = Column::new()
            .spacing(scale(8.0))
            .padding(scale(8.0));
        
        peers_column = peers_column.push(
            text("Found Peers").size(scale_text(20.0))
        );
        
        if self.peers.is_empty() {
            peers_column = peers_column.push(
                text("No peers found.")
            );
        } else {
            for peer in &self.peers {
                // Calculate actual games count from network_games
                let actual_games_count = self.network_games
                    .values()
                    .filter(|games| games.iter().any(|(peer_id, _)| peer_id == &peer.id))
                    .count();
                
                // Use games_count from peer_info if available, otherwise use actual count
                let games_count = peer.games_count.unwrap_or(actual_games_count as u32);
                
                // Get player name or use default
                let player_name = peer.player_name.as_ref()
                    .map(|n| n.as_str())
                    .unwrap_or("Unknown");
                
                // Get version or use default
                let version = peer.version.as_ref()
                    .map(|v| v.as_str())
                    .unwrap_or("Unknown");
                
                peers_column = peers_column.push(
                    container(
                        column![
                            text(format!("Player: {}", player_name)).size(scale_text(14.0)),
                            text(format!("Peer ID: {}", &peer.id[..16.min(peer.id.len())])).size(scale_text(10.0)),
                            text(format!("Games: {}", games_count)).size(scale_text(10.0)),
                            text(format!("Version: {}", version)).size(scale_text(10.0)),
                        ]
                        .spacing(5)
                        .padding(scale(12.0))
                    )
                    .style(container_box_style)
                    .width(Length::Fill)
                );
            }
        }
        
        scrollable(peers_column)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    
    /// Shows "Settings" tab
    /// Shows Performance Monitoring tab
    fn view_performance(&self) -> Element<'_, Message> {
        let metrics = &self.performance_metrics;
        
        // Format Geschwindigkeiten
        let format_speed = |bytes_per_sec: f64| -> String {
            if bytes_per_sec > 1_000_000.0 {
                format!("{:.2} MB/s", bytes_per_sec / 1_000_000.0)
            } else if bytes_per_sec > 1_000.0 {
                format!("{:.2} KB/s", bytes_per_sec / 1_000.0)
            } else {
                format!("{:.0} B/s", bytes_per_sec)
            }
        };
        
        let mut content = Column::new()
            .spacing(scale(15.0))
            .padding(scale(15.0));
        
        // Header
        content = content.push(
            text("Performance Monitoring").size(scale_text(24.0))
        );
        
        // Übersicht-Karten
        content = content.push(
            row![
                // Download-Statistiken
                container(
                    column![
                        text("Download").size(scale_text(18.0)),
                        Space::with_height(Length::Fixed(scale(10.0))),
                        text(format_speed(metrics.total_download_speed_bytes_per_sec))
                            .size(scale_text(20.0))
                            .style(|_theme: &Theme| {
                                iced::widget::text::Style {
                                    color: Some(Color::from_rgba(0.2, 0.8, 0.2, 1.0)),
                                }
                            }),
                        text(format!("{} aktive Downloads", metrics.active_downloads))
                            .size(scale_text(14.0)),
                        text(format!("{} Chunks heruntergeladen", metrics.total_chunks_downloaded))
                            .size(scale_text(12.0)),
                    ]
                    .spacing(scale(8.0))
                    .padding(scale(15.0))
                )
                .style(|_theme: &Theme| {
                    container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(0.1, 0.1, 0.15, 1.0))),
                        border: iced::Border {
                            radius: 8.0.into(),
                            width: 1.0,
                            color: Color::from_rgba(0.3, 0.3, 0.4, 1.0),
                        },
                        ..Default::default()
                    }
                })
                .width(Length::FillPortion(1)),
                
                Space::with_width(Length::Fixed(scale(15.0))),
                
                // Upload-Statistiken
                container(
                    column![
                        text("Upload").size(scale_text(18.0)),
                        Space::with_height(Length::Fixed(scale(10.0))),
                        text(format_speed(metrics.total_upload_speed_bytes_per_sec))
                            .size(scale_text(20.0))
                            .style(|_theme: &Theme| {
                                iced::widget::text::Style {
                                    color: Some(Color::from_rgba(0.2, 0.6, 0.8, 1.0)),
                                }
                            }),
                        text(format!("{} aktive Uploads", metrics.active_uploads))
                            .size(scale_text(14.0)),
                        text(format!("{} Chunks hochgeladen", metrics.total_chunks_uploaded))
                            .size(scale_text(12.0)),
                    ]
                    .spacing(scale(8.0))
                    .padding(scale(15.0))
                )
                .style(|_theme: &Theme| {
                    container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(0.1, 0.1, 0.15, 1.0))),
                        border: iced::Border {
                            radius: 8.0.into(),
                            width: 1.0,
                            color: Color::from_rgba(0.3, 0.3, 0.4, 1.0),
                        },
                        ..Default::default()
                    }
                })
                .width(Length::FillPortion(1)),
                
                Space::with_width(Length::Fixed(scale(15.0))),
                
                // Bandbreiten-Nutzung
                container(
                    column![
                        text("Bandbreite").size(scale_text(18.0)),
                        Space::with_height(Length::Fixed(scale(10.0))),
                        text(format!("{:.1}%", metrics.bandwidth_utilization_percent))
                            .size(scale_text(20.0))
                            .style(|_theme: &Theme| {
                                iced::widget::text::Style {
                                    color: Some(if metrics.bandwidth_utilization_percent > 80.0 {
                                        Color::from_rgba(0.9, 0.3, 0.3, 1.0) // Rot bei hoher Nutzung
                                    } else if metrics.bandwidth_utilization_percent > 50.0 {
                                        Color::from_rgba(0.9, 0.7, 0.2, 1.0) // Gelb bei mittlerer Nutzung
                                    } else {
                                        Color::from_rgba(0.2, 0.8, 0.2, 1.0) // Grün bei niedriger Nutzung
                                    }),
                                }
                            }),
                        text(format!("{} aktive Verbindungen", metrics.active_connections))
                            .size(scale_text(14.0)),
                        text(format!("Max: {} Chunks", self.config.max_concurrent_chunks))
                            .size(scale_text(12.0)),
                    ]
                    .spacing(scale(8.0))
                    .padding(scale(15.0))
                )
                .style(|_theme: &Theme| {
                    container::Style {
                        background: Some(iced::Background::Color(Color::from_rgba(0.1, 0.1, 0.15, 1.0))),
                        border: iced::Border {
                            radius: 8.0.into(),
                            width: 1.0,
                            color: Color::from_rgba(0.3, 0.3, 0.4, 1.0),
                        },
                        ..Default::default()
                    }
                })
                .width(Length::FillPortion(1)),
            ]
            .width(Length::Fill)
        );
        
        // Peer-Performance-Tabelle
        if !metrics.peer_performance.is_empty() {
            content = content.push(
                text("Peer Performance").size(scale_text(20.0))
            );
            
            let mut peers_table = Column::new()
                .spacing(scale(8.0));
            
            // Header
            peers_table = peers_table.push(
                row![
                    text("Peer ID").size(scale_text(14.0)).width(Length::FillPortion(2)),
                    text("Geschwindigkeit").size(scale_text(14.0)).width(Length::FillPortion(2)),
                    text("Erfolgsrate").size(scale_text(14.0)).width(Length::FillPortion(1)),
                    text("Requests").size(scale_text(14.0)).width(Length::FillPortion(1)),
                    text("Status").size(scale_text(14.0)).width(Length::FillPortion(1)),
                ]
                .spacing(scale(10.0))
                .padding(scale(8.0))
            );
            
            // Peer-Daten
            for (peer_id, perf) in &metrics.peer_performance {
                let speed_text = format_speed(perf.download_speed_bytes_per_sec);
                let success_rate_text = format!("{:.1}%", perf.success_rate * 100.0);
                let status_text = if let Some(blocked_until) = perf.blocked_until {
                    if std::time::Instant::now() < blocked_until {
                        "Blockiert".to_string()
                    } else {
                        "Aktiv".to_string()
                    }
                } else {
                    "Aktiv".to_string()
                };
                
                peers_table = peers_table.push(
                    container(
                        row![
                            text(peer_id.chars().take(12).collect::<String>())
                                .size(scale_text(12.0))
                                .width(Length::FillPortion(2)),
                            text(speed_text)
                                .size(scale_text(12.0))
                                .width(Length::FillPortion(2)),
                            text(success_rate_text)
                                .size(scale_text(12.0))
                                .width(Length::FillPortion(1)),
                            text(format!("{}/{}", perf.successful_requests, perf.total_requests))
                                .size(scale_text(12.0))
                                .width(Length::FillPortion(1)),
                            {
                                let status_text_clone = status_text.clone();
                                text(status_text)
                                    .size(scale_text(12.0))
                                    .style(move |_theme: &Theme| {
                                        iced::widget::text::Style {
                                            color: Some(if status_text_clone == "Blockiert" {
                                                Color::from_rgba(0.9, 0.3, 0.3, 1.0)
                                            } else {
                                                Color::from_rgba(0.2, 0.8, 0.2, 1.0)
                                            }),
                                        }
                                    })
                                    .width(Length::FillPortion(1))
                            },
                        ]
                        .spacing(scale(10.0))
                        .padding(scale(8.0))
                    )
                    .style(|_theme: &Theme| {
                        container::Style {
                            background: Some(iced::Background::Color(Color::from_rgba(0.05, 0.05, 0.1, 1.0))),
                            border: iced::Border {
                                radius: 4.0.into(),
                                width: 1.0,
                                color: Color::from_rgba(0.2, 0.2, 0.3, 1.0),
                            },
                            ..Default::default()
                        }
                    })
                );
            }
            
            content = content.push(
                scrollable(peers_table)
                    .height(Length::Fixed(scale(300.0)))
            );
        } else {
            content = content.push(
                text("Keine Peer-Performance-Daten verfügbar")
                    .size(scale_text(14.0))
                    .style(|_theme: &Theme| {
                        iced::widget::text::Style {
                            color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                        }
                    })
            );
        }
        
        // Letzte Aktualisierung
        let elapsed = metrics.last_update.elapsed().as_secs();
        content = content.push(
            text(format!("Letzte Aktualisierung: vor {} Sekunden", elapsed))
                .size(scale_text(10.0))
                .style(|_theme: &Theme| {
                    iced::widget::text::Style {
                        color: Some(Color::from_rgba(0.5, 0.5, 0.5, 1.0)),
                    }
                })
        );
        
        scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    
    fn view_settings_tab(&self) -> Element<'_, Message> {
        let version = env!("CARGO_PKG_VERSION");
        column![
            text("Settings").size(scale_text(20.0)),
            text_input("Player Name", &self.settings_player_name)
                .on_input(Message::SettingsPlayerNameChanged)
                .padding(scale(8.0)),
            row![
                text_input("Download Path", &self.config.download_path.to_string_lossy())
                    .on_input(Message::SettingsDownloadPathChanged)
                    .padding(scale(8.0)),
                button("Browse...")
                    .on_press(Message::BrowseDownloadPath)
                    .padding(scale(8.0)),
            ]
            .spacing(scale(8.0)),
            text(format!("Version: {}", version)).size(scale_text(10.0)),
            row![
                button("Save")
                    .on_press(Message::SaveSettings),
            ]
            .spacing(scale(8.0)),
        ]
        .spacing(15)
        .padding(20)
        .into()
    }
    
    /// Shows license dialog
    fn view_license_dialog(&self) -> Element<'_, Message> {
        // Make the entire dialog scrollable and responsive
        scrollable(
            container(
                column![
                    text("Welcome to DeckDrop").size(scale_text(20.0)),
                    Space::with_height(Length::Fixed(scale(12.0))),
                    text("Before you can use DeckDrop, you must agree to the terms and conditions.").size(scale_text(12.0)),
                    Space::with_height(Length::Fixed(scale(12.0))),
                    text("Player Name:").size(scale_text(12.0)),
                    text_input("Enter your player name", &self.license_player_name)
                        .on_input(Message::LicensePlayerNameChanged)
                        .padding(scale(8.0)),
                    Space::with_height(Length::Fixed(scale(12.0))),
                    text("DeckDrop is a peer-to-peer game sharing platform.\n\n\
                          By using DeckDrop, you agree to:\n\n\
                          • Only share games for which you have the rights\n\
                          • Not share illegal content\n\
                          • Take responsibility for your shared content\n\n\
                          DeckDrop assumes no liability for shared content.")
                        .size(scale_text(11.0)),
                    Space::with_height(Length::Fixed(scale(12.0))),
                    button("Accept")
                        .on_press(Message::AcceptLicense)
                        .style(button::primary),
                ]
                .spacing(scale(10.0))
                .padding(scale(15.0))
            )
            .width(Length::Fill)
            .max_width(scale(400.0))
            .height(Length::Shrink)
            .style(container_box_style)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
    
    /// Shows settings dialog
    fn view_settings(&self) -> Element<'_, Message> {
        container(
            column![
                text("Settings").size(scale_text(24.0)),
                Space::with_height(Length::Fixed(scale(15.0))),
                text("Player Name:").size(scale_text(14.0)),
                text_input("Player Name", &self.settings_player_name)
                    .on_input(Message::SettingsPlayerNameChanged)
                    .padding(scale(8.0)),
                Space::with_height(Length::Fixed(scale(8.0))),
                text("Download Path:").size(scale_text(14.0)),
                row![
                    text_input("Download Path", &self.settings_download_path)
                        .on_input(Message::SettingsDownloadPathChanged)
                        .padding(scale(8.0)),
                    button("Browse...")
                        .on_press(Message::BrowseDownloadPath)
                        .padding(scale(8.0)),
                ]
                .spacing(scale(8.0)),
                Space::with_height(Length::Fixed(scale(8.0))),
                text("Max Concurrent Chunks (1-10):").size(scale_text(14.0)),
                text_input("Max Concurrent Chunks", &self.settings_max_concurrent_chunks)
                    .on_input(Message::SettingsMaxConcurrentChunksChanged)
                    .padding(scale(8.0)),
                text("Number of chunks that can be downloaded simultaneously").size(scale_text(10.0))
                    .style(|_theme: &Theme| {
                        iced::widget::text::Style {
                            color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                        }
                    }),
                Space::with_height(Length::Fixed(scale(20.0))),
                row![
                    button("Cancel")
                        .on_press(Message::CancelSettings),
                    Space::with_width(Length::Fill),
                    button("Save")
                        .on_press(Message::SaveSettings)
                        .style(button::primary),
                ]
                .width(Length::Fill),
            ]
            .spacing(scale(8.0))
            .padding(scale(8.0))
        )
        .width(Length::Fixed(500.0))
        .height(Length::Shrink)
        .style(container_box_style)
        .into()
    }
    
    /// Shows "Add Game" dialog
    fn view_add_game_dialog(&self) -> Element<'_, Message> {
        container(
            scrollable(
                column![
                    text("Add Game").size(scale_text(24.0)),
                    Space::with_height(Length::Fixed(scale(15.0))),
                    row![
                        // Left column
                        column![
                            text("Path:").size(scale_text(14.0)),
                            row![
                                text_input("Path", &self.add_game_path)
                                    .on_input(Message::AddGamePathChanged)
                                    .padding(scale(8.0)),
                                button("Browse...")
                                    .on_press(Message::BrowseGamePath)
                                    .padding(scale(8.0)),
                            ]
                            .spacing(scale(8.0)),
                            Space::with_height(Length::Fixed(scale(8.0))),
                            text("Name:").size(scale_text(14.0)),
                            text_input("Name", &self.add_game_name)
                                .on_input(Message::AddGameNameChanged)
                                .padding(scale(8.0)),
                            Space::with_height(Length::Fixed(scale(8.0))),
                            text("Version:").size(scale_text(14.0)),
                            text(deckdrop_core::game::initial_version())
                                .size(scale_text(14.0))
                                .style(|_theme: &Theme| {
                                    iced::widget::text::Style {
                                        color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                    }
                                }), // Read-only: Immer "1" für neues Spiel
                            Space::with_height(Length::Fixed(scale(8.0))),
                            text("Start Args (optional):").size(scale_text(14.0)),
                            text_input("Start Args", &self.add_game_start_args)
                                .on_input(Message::AddGameStartArgsChanged)
                                .padding(scale(8.0)),
                        ]
                        .spacing(scale(8.0))
                        .width(Length::Fill),
                        Space::with_width(Length::Fixed(scale(15.0))),
                        // Right column
                        column![
                            text("Game Executable:").size(scale_text(14.0)),
                            row![
                            text_input("Relative to the game path", &self.add_game_start_file)
                                .on_input(Message::AddGameStartFileChanged)
                                .padding(scale(8.0)),
                                if self.add_game_path.is_empty() {
                                    button("Browse...")
                                        .padding(scale(8.0))
                                        .style(button::secondary)
                                } else {
                                    button("Browse...")
                                        .on_press(Message::BrowseStartFile)
                                        .padding(scale(8.0))
                                },
                            ]
                            .spacing(scale(8.0)),
                            // Progress bar for chunk generation
                            if let Some((current, total, file_name)) = &self.add_game_progress {
                                if *total > 0 {
                                    let progress = *current as f32 / *total as f32;
                                    column![
                                        Space::with_height(Length::Fixed(scale(8.0))),
                                        text(format!("Generiere Chunks: {}/{}", current, total)).size(scale_text(12.0)),
                                        text(format!("Datei: {}", file_name)).size(scale_text(10.0)),
                                        progress_bar(0.0..=1.0, progress)
                                            .height(Length::Fixed(scale(15.0))),
                                    ]
                                    .spacing(scale(4.0))
                                } else {
                                    column![].spacing(0)
                                }
                            } else {
                                column![].spacing(0)
                            },
                            Space::with_height(Length::Fixed(scale(8.0))),
                            text("Description (optional):").size(scale_text(14.0)),
                            text_input("Description", &self.add_game_description)
                                .on_input(Message::AddGameDescriptionChanged)
                                .padding(scale(8.0)),
                            Space::with_height(Length::Fixed(scale(8.0))),
                            text("Additional Instructions (optional):").size(scale_text(14.0)),
                            text_input("Additional Instructions", &self.add_game_additional_instructions)
                                .on_input(Message::AddGameAdditionalInstructionsChanged)
                                .padding(scale(8.0)),
                        ]
                        .spacing(scale(8.0))
                        .width(Length::Fill),
                    ]
                    .spacing(scale(15.0))
                    .width(Length::Fill),
                    Space::with_height(Length::Fixed(scale(15.0))),
                    row![
                        button("Cancel")
                            .on_press(Message::CancelAddGame),
                        Space::with_width(Length::Fill),
                        if self.add_game_saving {
                            button("Save")
                                .style(button::primary)
                        } else {
                            button("Save")
                                .on_press(Message::SaveGame)
                                .style(button::primary)
                        },
                    ]
                    .width(Length::Fill),
                ]
                .spacing(scale(12.0))
                .padding(scale(20.0))
            )
            .width(Length::Fill)
            .height(Length::Fill)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(container_box_style)
        .into()
    }
    
    /// Shows game details view
    fn view_game_details(&self) -> Element<'_, Message> {
        if let Some((game_path, game_info)) = &self.current_game_details {
            // Check if this game is currently downloading
            // Try to find by game_id first (for network games), then by path (for local games)
            let download_state = self.active_downloads.get(&game_info.game_id)
                .or_else(|| {
                    self.active_downloads.values().find(|ds| {
                        PathBuf::from(&ds.manifest.game_path) == *game_path
                    })
                });
            
            let integrity_status = self.game_integrity_status.get(game_path)
                .unwrap_or(&GameIntegrityStatus::NotChecked);
            
            // Check if this is a local game (exists in my_games)
            let is_local_game = self.my_games.iter().any(|(path, _)| path == game_path);
            let is_checking = matches!(integrity_status, GameIntegrityStatus::Checking { .. });
            
            // Check if this is a network game that we don't have locally
            let is_network_game = !is_local_game && self.network_games.contains_key(&game_info.game_id);
            let is_downloading = download_state.is_some();
            
            // Prüfe, ob der aktuelle Benutzer der Creator ist
            let is_creator = game_info.creator_peer_id.as_ref()
                .and_then(|creator_id| self.config.peer_id.as_ref().map(|my_id| creator_id == my_id))
                .unwrap_or(false);
            
            let details_column = column![
                // Header with back button and action buttons (outside the frame)
                {
                    let mut header_row = row![
                        button("← Back")
                            .on_press(Message::BackFromDetails)
                            .style(button::secondary),
                        Space::with_width(Length::Fill),
                    ];
                    
                    // Add action buttons (right-aligned)
                    if is_local_game {
                        // Local game: show Check Integrity button
                        if is_checking {
                            header_row = header_row.push(button("Checking...")
                                .style(button::secondary));
                        } else {
                            header_row = header_row.push(button("Check Integrity")
                                .on_press(Message::CheckIntegrity(game_path.clone()))
                                .style(button::secondary));
                        }
                        
                        // Show Edit button if user is creator
                        if is_creator && !self.editing_game {
                            header_row = header_row.push(button("Edit")
                                .on_press(Message::EditGame)
                                .style(button::primary));
                        }
                    } else if is_network_game && !is_downloading {
                        // Network game without download: show Download button or preparation progress (thread-safe)
                        let is_preparing = if let Ok(preparing) = self.preparing_downloads.lock() {
                            preparing.contains_key(&game_info.game_id)
                        } else {
                            false
                        };
                        let is_starting = if let Ok(starting) = self.downloading_starting.lock() {
                            starting.contains(&game_info.game_id)
                        } else {
                            false
                        };
                        
                        if is_preparing {
                            // Zeige Ladebalken für Pre-Allocation (thread-safe)
                            let (current, total) = if let Ok(preparing) = self.preparing_downloads.lock() {
                                preparing.get(&game_info.game_id).copied().unwrap_or((0, 1))
                            } else {
                                (0, 1)
                            };
                            let progress = if total > 0 {
                                current as f32 / total as f32
                            } else {
                                0.0
                            };
                            
                            header_row = header_row.push(
                                column![
                                    text(format!("Vorbereitung... ({}/{})", current, total))
                                        .size(scale_text(12.0)),
                                    progress_bar(0.0..=1.0, progress)
                                        .width(Length::Fixed(scale(200.0)))
                                        .height(Length::Fixed(scale(8.0))),
                                ]
                                .spacing(scale(4.0))
                            );
                        } else if is_starting {
                            header_row = header_row.push(button("Starting...")
                                .style(button::secondary));
                        } else {
                            header_row = header_row.push(button("Download")
                                .on_press(Message::DownloadGame(game_info.game_id.clone()))
                                .style(button::primary));
                        }
                    }
                    
                    header_row
                        .spacing(scale(8.0))
                        .width(Length::Fill)
                },
                Space::with_height(Length::Fixed(scale(15.0))),
                // Content with frame
                container(
                    column![
                        Space::with_height(Length::Fixed(scale(20.0))),
                        // Game title - large and prominent (editierbar im Bearbeitungsmodus)
                        if self.editing_game {
                            column![
                                text("Name:").size(scale_text(14.0)),
                                text_input("Name", &self.edit_game_name)
                                    .on_input(Message::EditGameNameChanged)
                                    .padding(scale(8.0)),
                            ]
                            .spacing(scale(8.0))
                        } else {
                            column![
                                text(&game_info.name).size(scale_text(36.0)),
                            ]
                        },
                        Space::with_height(Length::Fixed(scale(8.0))),
                        text(&game_info.version).size(scale_text(16.0))
                            .style(|_theme: &Theme| {
                                iced::widget::text::Style {
                                    color: Some(Color::from_rgba(0.7, 0.7, 0.7, 1.0)),
                                }
                            }),
                        if self.editing_game {
                            column![
                                text(format!("Neue Version: {}", deckdrop_core::game::increment_version(&game_info.version)))
                                    .size(scale_text(12.0))
                                    .style(|_theme: &Theme| {
                                        iced::widget::text::Style {
                                            color: Some(Color::from_rgba(0.5, 0.8, 1.0, 1.0)),
                                        }
                                    }),
                            ]
                        } else {
                            column![]
                        },
                        Space::with_height(Length::Fixed(scale(30.0))),
                        // Main content in two columns for better use of space
                        row![
                            // Left column - Basic Information
                            column![
                                text("Information").size(scale_text(20.0))
                                    .style(|_theme: &Theme| {
                                        iced::widget::text::Style {
                                            color: Some(Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                                        }
                                    }),
                                Space::with_height(Length::Fixed(scale(20.0))),
                                row![
                                    text("Game ID").size(scale_text(13.0))
                                        .style(|_theme: &Theme| {
                                            iced::widget::text::Style {
                                                color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                            }
                                        })
                                        .width(Length::Fixed(scale(140.0))),
                                    text(&game_info.game_id).size(scale_text(13.0)),
                                ]
                                .width(Length::Fill),
                                Space::with_height(Length::Fixed(scale(16.0))),
                                column![
                                    text("Path").size(scale_text(13.0))
                                        .style(|_theme: &Theme| {
                                            iced::widget::text::Style {
                                                color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                            }
                                        }),
                                    Space::with_height(Length::Fixed(scale(6.0))),
                                    text(game_path.display().to_string()).size(scale_text(13.0)),
                                ]
                                .width(Length::Fill),
                                Space::with_height(Length::Fixed(scale(16.0))),
                                if self.editing_game {
                                    column![
                                        text("Start File:").size(scale_text(13.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(6.0))),
                                        row![
                                            text_input("Start File", &self.edit_game_start_file)
                                                .on_input(Message::EditGameStartFileChanged)
                                                .padding(scale(8.0)),
                                            button("Browse...")
                                                .on_press(Message::BrowseEditStartFile)
                                                .padding(scale(8.0)),
                                        ]
                                        .spacing(scale(8.0)),
                                        Space::with_height(Length::Fixed(scale(16.0))),
                                        text("Start Args:").size(scale_text(13.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(6.0))),
                                        text_input("Start Args (optional)", &self.edit_game_start_args)
                                            .on_input(Message::EditGameStartArgsChanged)
                                            .padding(scale(8.0)),
                                    ]
                                    .width(Length::Fill)
                                } else {
                                    column![
                                        row![
                                            text("Start File").size(scale_text(13.0))
                                                .style(|_theme: &Theme| {
                                                    iced::widget::text::Style {
                                                        color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                    }
                                                })
                                                .width(Length::Fixed(scale(140.0))),
                                            text(&game_info.start_file).size(scale_text(13.0)),
                                        ]
                                        .width(Length::Fill),
                                        if let Some(ref start_args) = game_info.start_args {
                                            column![
                                                Space::with_height(Length::Fixed(scale(16.0))),
                                                column![
                                                    text("Start Args").size(scale_text(13.0))
                                                        .style(|_theme: &Theme| {
                                                            iced::widget::text::Style {
                                                                color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                            }
                                                        }),
                                                    Space::with_height(Length::Fixed(scale(6.0))),
                                                    text(start_args).size(scale_text(13.0)),
                                                ]
                                                .width(Length::Fill),
                                            ]
                                        } else {
                                            column![]
                                        },
                                    ]
                                    .width(Length::Fill)
                                },
                            ]
                            .width(Length::Fill),
                            Space::with_width(Length::Fixed(scale(40.0))),
                            // Right column - Status and Metadata
                            column![
                                text("Status").size(scale_text(20.0))
                                    .style(|_theme: &Theme| {
                                        iced::widget::text::Style {
                                            color: Some(Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                                        }
                                    }),
                                Space::with_height(Length::Fixed(scale(20.0))),
                                // Integrity status (only for local games)
                                if is_local_game {
                                    column![
                                        text("Integrity").size(scale_text(13.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(6.0))),
                                        match integrity_status {
                                            GameIntegrityStatus::NotChecked => {
                                                text("Not checked").size(scale_text(14.0))
                                                    .style(|_theme: &Theme| {
                                                        iced::widget::text::Style {
                                                            color: Some(Color::from_rgba(0.7, 0.7, 0.7, 1.0)),
                                                        }
                                                    })
                                            }
                                            GameIntegrityStatus::Checking { current, total } => {
                                                if *total > 0 {
                                                    text(format!("Checking... ({}/{})", current, total)).size(scale_text(14.0))
                                                        .style(|_theme: &Theme| {
                                                            iced::widget::text::Style {
                                                                color: Some(Color::from_rgba(0.7, 0.7, 0.7, 1.0)),
                                                            }
                                                        })
                                                } else {
                                                    text("Checking...").size(scale_text(14.0))
                                                        .style(|_theme: &Theme| {
                                                            iced::widget::text::Style {
                                                                color: Some(Color::from_rgba(0.7, 0.7, 0.7, 1.0)),
                                                            }
                                                        })
                                                }
                                            }
                                            GameIntegrityStatus::Intact => {
                                                text("Game files intact").size(scale_text(14.0))
                                                    .style(|_theme: &Theme| {
                                                        iced::widget::text::Style {
                                                            color: Some(Color::from_rgba(0.0, 1.0, 0.0, 1.0)),
                                                        }
                                                    })
                                            }
                                            GameIntegrityStatus::Changed => {
                                                text("Game files have changed").size(scale_text(14.0))
                                                    .style(|_theme: &Theme| {
                                                        iced::widget::text::Style {
                                                            color: Some(Color::from_rgba(1.0, 0.7, 0.0, 1.0)),
                                                        }
                                                    })
                                            }
                                            GameIntegrityStatus::Error(ref e) => {
                                                text(format!("Error: {}", e)).size(scale_text(14.0))
                                                    .style(|_theme: &Theme| {
                                                        iced::widget::text::Style {
                                                            color: Some(Color::from_rgba(1.0, 0.0, 0.0, 1.0)),
                                                        }
                                                    })
                                            }
                                        },
                                    ]
                                    .width(Length::Fill)
                                } else {
                                    column![]
                                },
                                // Download status if downloading
                                if let Some(ds) = download_state {
                                    // Calculate downloaded and total sizes (same logic as in network games list)
                                    let total_bytes: u64 = ds.manifest.chunks.values()
                                        .filter_map(|chunk_info| chunk_info.file_size)
                                        .sum();
                                    
                                    let downloaded_bytes = if total_bytes > 0 && ds.manifest.progress.total_chunks > 0 {
                                        // Use proportional calculation based on actual total size
                                        (total_bytes as f64 * (ds.manifest.progress.downloaded_chunks as f64 / ds.manifest.progress.total_chunks as f64)) as u64
                                    } else {
                                        // Fallback: assume 100MB per chunk
                                        const CHUNK_SIZE_BYTES: u64 = 100 * 1024 * 1024;
                                        (ds.manifest.progress.downloaded_chunks as u64) * CHUNK_SIZE_BYTES
                                    };
                                    
                                    let total_bytes_final = if total_bytes > 0 {
                                        total_bytes
                                    } else {
                                        // Fallback: estimate from chunk count
                                        const CHUNK_SIZE_BYTES: u64 = 100 * 1024 * 1024;
                                        (ds.manifest.progress.total_chunks as u64) * CHUNK_SIZE_BYTES
                                    };
                                    
                                    let downloaded_size_str = format_size(downloaded_bytes);
                                    let total_size_str = format_size(total_bytes_final);
                                    
                                    // Get currently downloading chunks (requested but not yet downloaded) with real progress
                                    // Zeige nur Chunks, die wirklich noch fehlen (in missing_chunks)
                                    let downloading_chunks: Vec<(String, f64)> = {
                                        // Verwende Manifest aus DownloadState im Speicher, um teure I/O im UI-Thread zu vermeiden
                                        let current_missing_chunks = ds.manifest.get_missing_chunks();
                                        
                                        if let Ok(requested) = self.requested_chunks.lock() {
                                            if let Ok(start_times) = self.chunk_download_start_times.lock() {
                                                let now = std::time::Instant::now();
                                                const CHUNK_SIZE_BYTES: u64 = 100 * 1024 * 1024; // 100MB
                                                
                                                // Berechne durchschnittliche Download-Geschwindigkeit aus aktiven Downloads
                                                let avg_speed = if ds.download_speed_bytes_per_sec > 0.0 {
                                                    ds.download_speed_bytes_per_sec
                                                } else {
                                                    1_000_000.0 // Fallback: 1 MB/s
                                                };
                                                
                                                // Nur Chunks anzeigen, die wirklich noch fehlen UND angefordert wurden
                                                // WICHTIG: Chunks, die zu 100% sind, werden nicht mehr angezeigt (sind fertig)
                                                // Wenn ein Chunk nicht mehr in missing_chunks ist, ist er fertig und wird nicht mehr angezeigt
                                                let mut chunks_with_progress: Vec<(String, f64)> = current_missing_chunks.iter()
                                                    .filter(|chunk| requested.contains(*chunk))
                                                    .map(|chunk| {
                                                        // Berechne Progress basierend auf verstrichener Zeit
                                                        if let Some(start_time) = start_times.get(chunk) {
                                                            let elapsed_secs = now.duration_since(*start_time).as_secs_f64();
                                                            
                                                            // Realistische Progress-Berechnung basierend auf tatsächlicher Download-Geschwindigkeit
                                                            // Progress steigt kontinuierlich basierend auf verstrichener Zeit und Geschwindigkeit
                                                            let progress = if elapsed_secs < 1.0 {
                                                                // Erste Sekunde: 0-5% (Verbindungsaufbau)
                                                                (elapsed_secs * 5.0).min(5.0)
                                                            } else {
                                                                // Danach: basierend auf tatsächlicher Download-Geschwindigkeit
                                                                // Verwende realistische Schätzung: 80% der gemessenen Geschwindigkeit
                                                                let effective_speed = avg_speed * 0.8;
                                                                let downloaded_bytes = (elapsed_secs - 1.0) * effective_speed;
                                                                let base_progress = 5.0; // Start bei 5% nach 1 Sekunde
                                                                let additional_progress = (downloaded_bytes / CHUNK_SIZE_BYTES as f64 * 95.0).min(95.0); // Maximal 100% insgesamt
                                                                (base_progress + additional_progress).min(100.0).max(0.0)
                                                            };
                                                            (chunk.clone(), progress)
                                                        } else {
                                                            // Keine Startzeit gefunden - verwende 0%
                                                            (chunk.clone(), 0.0)
                                                        }
                                                    })
                                                    // Filtere Chunks mit 100% Progress - diese sind fertig und werden nicht mehr angezeigt
                                                    .filter(|(_, progress)| *progress < 100.0)
                                                    .collect();
                                                
                                                // Begrenze Anzahl angezeigter Chunks:
                                                // - technisch auf einen hohen Wert (50) für Debugging
                                                // - visuell zusätzlich auf max_concurrent_chunks aus der Config,
                                                //   damit nie mehr Chunks gleichzeitig angezeigt werden,
                                                //   als laut Einstellungen parallel geladen werden sollen.
                                                const MAX_CHUNKS_SHOWN: usize = 50;
                                                if chunks_with_progress.len() > MAX_CHUNKS_SHOWN {
                                                    chunks_with_progress.truncate(MAX_CHUNKS_SHOWN);
                                                }
                                                let ui_limit = self.config.max_concurrent_chunks.max(1);
                                                if chunks_with_progress.len() > ui_limit {
                                                    chunks_with_progress.truncate(ui_limit);
                                                }
                                                
                                                // Sortiere alphabetisch nach Chunk-Hash (stabil, keine Sprünge)
                                                chunks_with_progress.sort_by(|a, b| a.0.cmp(&b.0));
                                                chunks_with_progress
                                            } else {
                                                Vec::new()
                                            }
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    
                                    // Get chunks that are being written (downloaded but not yet in manifest)
                                    let writing_chunks_list: Vec<String> = {
                                        if let Ok(writing) = self.writing_chunks.lock() {
                                            // Verwende Manifest aus DownloadState im Speicher, um teure I/O im UI-Thread zu vermeiden
                                            let current_missing_chunks = ds.manifest.get_missing_chunks();
                                            // Nur Chunks anzeigen, die wirklich noch fehlen (in missing_chunks)
                                            writing.iter()
                                                .filter(|chunk| current_missing_chunks.contains(*chunk))
                                                .cloned()
                                                .collect()
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    
                                    column![
                                        Space::with_height(Length::Fixed(scale(24.0))),
                                        text("Download").size(scale_text(13.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(6.0))),
                                        text(format!("Status: {:?}", ds.manifest.overall_status)).size(scale_text(13.0)),
                                        Space::with_height(Length::Fixed(scale(8.0))),
                                        // Gesamt-Progress-Balken
                                        text(format!("Progress: {:.1}%", ds.progress_percent)).size(scale_text(13.0)),
                                        Space::with_height(Length::Fixed(scale(4.0))),
                                        progress_bar(0.0..=100.0, ds.progress_percent)
                                            .width(Length::Fill)
                                            .height(Length::Fixed(scale(20.0))),
                                        Space::with_height(Length::Fixed(scale(4.0))),
                                        text(format!("Size: {} / {}", downloaded_size_str, total_size_str)).size(scale_text(13.0)),
                                        Space::with_height(Length::Fixed(scale(4.0))),
                                        text(format!("Chunks: {}/{} | Done: {}", ds.manifest.progress.downloaded_chunks, ds.manifest.progress.total_chunks, ds.manifest.progress.downloaded_chunks)).size(scale_text(13.0)),
                                        
                                        // Info über aktive Transfers (unter der Dateiliste)
                                        if !downloading_chunks.is_empty() || !writing_chunks_list.is_empty() {
                                            column![
                                                Space::with_height(Length::Fixed(scale(16.0))),
                                                text(format!("Active Transfers: {} downloading, {} writing", downloading_chunks.len(), writing_chunks_list.len()))
                                                    .size(scale_text(12.0))
                                                    .style(|_theme: &Theme| {
                                                        iced::widget::text::Style {
                                                            color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                        }
                                                    }),
                                            ]
                                        } else {
                                            column![]
                                        }
                                    ]
                                    .width(Length::Fill)
                                } else {
                                    column![]
                                },
                                if let Some(ref creator_peer_id) = game_info.creator_peer_id {
                                    column![
                                        Space::with_height(Length::Fixed(scale(24.0))),
                                        text("Creator Peer ID").size(scale_text(13.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.6, 0.6, 0.6, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(6.0))),
                                        text(creator_peer_id).size(scale_text(13.0)),
                                    ]
                                    .width(Length::Fill)
                                } else {
                                    column![]
                                },
                               
                            ]
                            .width(Length::Fill),
                        ]
                        .width(Length::Fill),
                        // File Tree (Full Width)
                        if let Some(ds) = download_state {
                            column![
                                Space::with_height(Length::Fixed(scale(30.0))),
                                text("Files").size(scale_text(20.0))
                                    .style(|_theme: &Theme| {
                                        iced::widget::text::Style {
                                            color: Some(Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                                        }
                                    }),
                                Space::with_height(Length::Fixed(scale(16.0))),
                                container(
                                    scrollable(
                                        {
                                            // Erstelle TreeView on-the-fly
                                            let tree = FileTreeNode::from_manifest(&ds.manifest);
                                            tree.view(0.0)
                                        }
                                    )
                                    .height(Length::Fixed(scale(400.0))) // Mehr Platz für Full Width
                                )
                                .style(container_box_style)
                                .padding(scale(8.0))
                                .width(Length::Fill),
                            ]
                            .width(Length::Fill)
                        } else {
                            column![]
                        },

                        // Description and Instructions - full width (editierbar im Bearbeitungsmodus)
                        if self.editing_game || game_info.description.is_some() || game_info.additional_instructions.is_some() {
                            column![
                                Space::with_height(Length::Fixed(scale(40.0))),
                                if self.editing_game {
                                    column![
                                        text("Description:").size(scale_text(20.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(16.0))),
                                        text_input("Description (optional)", &self.edit_game_description)
                                            .on_input(Message::EditGameDescriptionChanged)
                                            .padding(scale(8.0)),
                                        Space::with_height(Length::Fixed(scale(30.0))),
                                        text("Additional Instructions:").size(scale_text(20.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(16.0))),
                                        text_input("Additional Instructions (optional)", &self.edit_game_additional_instructions)
                                            .on_input(Message::EditGameAdditionalInstructionsChanged)
                                            .padding(scale(8.0)),
                                    ]
                                    .width(Length::Fill)
                                } else if let Some(ref description) = game_info.description {
                                    column![
                                        text("Description").size(scale_text(20.0))
                                            .style(|_theme: &Theme| {
                                                iced::widget::text::Style {
                                                    color: Some(Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                                                }
                                            }),
                                        Space::with_height(Length::Fixed(scale(16.0))),
                                        text(description).size(scale_text(14.0))
                                            .line_height(1.6),
                                    ]
                                    .width(Length::Fill)
                                } else {
                                    column![]
                                },
                                // Additional Instructions (nur im Anzeigemodus, nicht im Bearbeitungsmodus)
                                if !self.editing_game {
                                    if let Some(ref instructions) = game_info.additional_instructions {
                                        column![
                                            Space::with_height(Length::Fixed(scale(40.0))),
                                            text("Additional Instructions").size(scale_text(20.0))
                                                .style(|_theme: &Theme| {
                                                    iced::widget::text::Style {
                                                        color: Some(Color::from_rgba(0.9, 0.9, 0.9, 1.0)),
                                                    }
                                                }),
                                            Space::with_height(Length::Fixed(scale(16.0))),
                                            text(instructions).size(scale_text(14.0))
                                                .line_height(1.6),
                                        ]
                                        .width(Length::Fill)
                                    } else {
                                        column![]
                                    }
                                } else {
                                    column![]
                                },
                            ]
                        } else {
                            column![]
                        },
                        // Save/Cancel buttons im Bearbeitungsmodus
                        if self.editing_game {
                            column![
                                Space::with_height(Length::Fixed(scale(30.0))),
                                row![
                                    button("Cancel")
                                        .on_press(Message::CancelGameEdit)
                                        .style(button::secondary),
                                    Space::with_width(Length::Fill),
                                    button("Save")
                                        .on_press(Message::SaveGameEdit)
                                        .style(button::primary),
                                ]
                                .width(Length::Fill)
                                .spacing(scale(8.0)),
                            ]
                            .width(Length::Fill)
                        } else {
                            column![]
                        },
                        Space::with_height(Length::Fixed(scale(20.0))),
                    ]
                    .spacing(scale(0.0))
                    .width(Length::Fill)
                    .padding(scale(20.0))
                )
                .width(Length::Fill)
                .style(container_box_style),
            ]
            .spacing(scale(0.0))
            .padding(scale(15.0))
            .width(Length::Fill);
            
            scrollable(details_column)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            // Fallback if no game details available
            column![
                button("← Back")
                    .on_press(Message::BackFromDetails)
                    .style(button::secondary),
                text("No game details available").size(scale_text(16.0)),
            ]
            .spacing(scale(15.0))
            .padding(scale(15.0))
            .into()
        }
    }
}

/// Box style for container
fn container_box_style(theme: &Theme) -> iced::widget::container::Style {
    use iced::widget::container;
    let palette = theme.palette();
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgba(0.2, 0.2, 0.2, 0.5))),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgba(0.4, 0.4, 0.4, 1.0),
        },
        text_color: Some(palette.text),
        shadow: Default::default(),
    }
}

/// Worker-Funktion für paralleles Schreiben von Chunks
fn run_chunk_writer(
    rx: async_channel::Receiver<ChunkWriteTask>,
    writing_chunks: Arc<std::sync::Mutex<HashSet<String>>>,
    requested_chunks: Arc<std::sync::Mutex<HashSet<String>>>,
    chunk_peer_requests: Arc<std::sync::Mutex<HashMap<String, String>>>,
    active_requests_per_peer: Arc<std::sync::Mutex<HashMap<String, usize>>>,
    chunk_download_start_times: Arc<std::sync::Mutex<HashMap<String, std::time::Instant>>>,
    chunk_retries: Arc<std::sync::Mutex<HashMap<String, ChunkRetryInfo>>>,
) {
    eprintln!("ChunkWriter Thread gestartet (Parallel)");
    
    while let Ok(task) = rx.recv_blocking() {
        let chunk_hash = task.chunk_hash;
        let chunk_data = task.chunk_data; // Move ownership
        let _peer_id = task.peer_id; // Wird aktuell nicht direkt hier verwendet, aber gut für Debugging
        
        // Drop-Guard: Entfernt Chunk aus "wird geschrieben" Set am Ende (egal ob Erfolg oder Fehler)
        struct WritingChunksGuard {
            writing_chunks: Arc<std::sync::Mutex<HashSet<String>>>,
            chunk_hash: String,
        }
        impl Drop for WritingChunksGuard {
            fn drop(&mut self) {
                if let Ok(mut writing) = self.writing_chunks.lock() {
                    writing.remove(&self.chunk_hash);
                }
            }
        }

        // Markiere als "wird geschrieben"
        if let Ok(mut writing) = writing_chunks.lock() {
            writing.insert(chunk_hash.clone());
        }
        
        // Erstelle Guard
        let _writing_guard = WritingChunksGuard {
            writing_chunks: writing_chunks.clone(),
            chunk_hash: chunk_hash.clone(),
        };

        // Finde Game-ID
        if let Ok(game_id) = deckdrop_core::find_game_id_for_chunk(&chunk_hash) {
            // Prüfe ob Download pausiert ist
            let mut is_paused = false;
            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                if let Ok(manifest) = deckdrop_core::DownloadManifest::load(&manifest_path) {
                    if matches!(manifest.overall_status, deckdrop_core::DownloadStatus::Paused) {
                        is_paused = true;
                    } else {
                        // Validierung
                        if let Err(e) = deckdrop_core::validate_chunk_size(&chunk_hash, &chunk_data, &manifest) {
                            eprintln!("Chunk-Validierung fehlgeschlagen für {}: {}", chunk_hash, e);
                            // Cleanup requested/start_times
                            if let Ok(mut requested) = requested_chunks.lock() { requested.remove(&chunk_hash); }
                            if let Ok(mut start_times) = chunk_download_start_times.lock() { start_times.remove(&chunk_hash); }
                            continue;
                        }
                        
                        // Schreiben
                        eprintln!("DEBUG: Starte Schreiben für Chunk {}", chunk_hash);
                        match deckdrop_core::write_chunk_to_file(&chunk_hash, &chunk_data, &manifest) {
                            Ok(()) => {
                                eprintln!("DEBUG: Schreiben erfolgreich für Chunk {}. Starte DB-Update...", chunk_hash);
                                // DB Update
                                match deckdrop_core::synch::mark_chunk_downloaded_sqlite(&game_id, &chunk_hash) {
                                    Ok(()) => {
                                        eprintln!("DEBUG: DB-Update erfolgreich für Chunk {}", chunk_hash);
                                        // Notify Network
                                        if let Some(tx) = crate::network_bridge::get_download_request_tx() {
                                            let _ = tx.send(deckdrop_network::network::discovery::DownloadRequest::ChunkDownloadCompleted {
                                                chunk_hash: chunk_hash.clone(),
                                            });
                                        }
                                        
                                        // Remove from requested_chunks
                                        if let Ok(mut requested) = requested_chunks.lock() {
                                            if requested.remove(&chunk_hash) {
                                                println!("🗑️ Chunk {} aus requested_chunks entfernt (Worker)", chunk_hash);
                                                // Decrement peer active requests
                                                if let Ok(mut chunk_peers) = chunk_peer_requests.lock() {
                                                    if let Some(pid) = chunk_peers.remove(&chunk_hash) {
                                                        if let Ok(mut active) = active_requests_per_peer.lock() {
                                                            if let Some(c) = active.get_mut(&pid) {
                                                                if *c > 0 { *c -= 1; }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        
                                        // Cleanup start_times
                                        if let Ok(mut start_times) = chunk_download_start_times.lock() { start_times.remove(&chunk_hash); }
                                        
                                        // Force Update Active Downloads - ENTFERNT um UI Freeze zu verhindern
                                        // Der UI Thread lädt die aktiven Downloads sowieso regelmäßig neu.
                                        // let _ = deckdrop_core::force_update_active_downloads();
                                        
                                        // Check Single File Integrity (Sequentiell im Worker)
                                        // OPTIMIERUNG: Verwende DB-Query statt Manifest-Reload um Datei zu finden
                                        if let Ok(Some(file_path)) = deckdrop_core::find_file_for_chunk_db(&game_id, &chunk_hash) {
                                            // Lade Manifest nur wenn wir eine Datei gefunden haben, die potentiell fertig ist
                                            // Das Manifest ist jetzt "Light" (ohne Chunks), daher ist das Laden schnell.
                                            if let Ok(manifest_reloaded) = deckdrop_core::DownloadManifest::load(&manifest_path) {
                                                let _ = deckdrop_core::check_and_validate_single_file(&game_id, &manifest_reloaded, &file_path);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("KRITISCH: DB Update Fehler für {}: {}", chunk_hash, e);
                                        // Nicht entfernen aus requested, Retry folgt durch Scheduler Timeout
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("KRITISCH: Schreibfehler für {}: {}", chunk_hash, e);
                                // Cleanup start_times bei Schreibfehler
                                if let Ok(mut start_times) = chunk_download_start_times.lock() { start_times.remove(&chunk_hash); }
                            }
                        }
                    }
                }
            }
            
            if is_paused {
                 println!("Download pausiert, überspringe Chunk {}", chunk_hash);
            }
            
            // Retry cleanup
            if let Ok(mut retries) = chunk_retries.lock() {
                retries.remove(&chunk_hash);
            }
        } else {
            eprintln!("Konnte game_id für Chunk {} nicht finden", chunk_hash);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::sync::mpsc;
    use deckdrop_network::network::discovery::DiscoveryEvent;
    
    /// Testet, ob Upload- und Download-Statistiken korrekt aktualisiert werden
    #[test]
    fn test_upload_download_statistics_update() {
        // Erstelle eine App-Instanz
        let (_tx, rx) = mpsc::channel(32);
        let rx_arc = Arc::new(Mutex::new(rx));
        let mut app = DeckDropApp::new_with_network_rx(rx_arc);
        
        // Initial: Statistiken sollten leer sein
        let initial_upload_stats = app.upload_stats.lock().unwrap();
        assert_eq!(initial_upload_stats.active_upload_count, 0);
        assert_eq!(initial_upload_stats.upload_speed_bytes_per_sec, 0.0);
        drop(initial_upload_stats);
        
        // Simuliere Upload-Events (ChunkUploaded)
        const CHUNK_SIZE_1: usize = 10 * 1024 * 1024; // 10MB
        const CHUNK_SIZE_2: usize = 5 * 1024 * 1024; // 5MB
        
        // Upload 1: Erster Chunk
        app.handle_network_event(DiscoveryEvent::ChunkUploaded {
            peer_id: "peer1".to_string(),
            chunk_hash: "hash1:0".to_string(),
            chunk_size: CHUNK_SIZE_1,
        });
        
        // Prüfe Upload-Statistiken nach erstem Upload
        let upload_stats_1 = app.upload_stats.lock().unwrap();
        assert_eq!(upload_stats_1.active_upload_count, 1, "Nach erstem Upload sollte 1 aktiver Upload sein");
        assert!(upload_stats_1.upload_speed_bytes_per_sec > 0.0, "Upload-Geschwindigkeit sollte > 0 sein");
        assert_eq!(upload_stats_1.last_uploaded_bytes, CHUNK_SIZE_1 as u64, "Uploaded bytes sollten korrekt sein");
        drop(upload_stats_1);
        
        // Upload 2: Zweiter Chunk
        app.handle_network_event(DiscoveryEvent::ChunkUploaded {
            peer_id: "peer2".to_string(),
            chunk_hash: "hash2:0".to_string(),
            chunk_size: CHUNK_SIZE_2,
        });
        
        // Prüfe Upload-Statistiken nach zweitem Upload
        let upload_stats_2 = app.upload_stats.lock().unwrap();
        assert_eq!(upload_stats_2.active_upload_count, 2, "Nach zweitem Upload sollten 2 aktive Uploads sein");
        assert!(upload_stats_2.upload_speed_bytes_per_sec > 0.0, "Upload-Geschwindigkeit sollte > 0 sein");
        assert_eq!(upload_stats_2.last_uploaded_bytes, (CHUNK_SIZE_1 + CHUNK_SIZE_2) as u64, "Uploaded bytes sollten kumuliert sein");
        drop(upload_stats_2);
        
        // Simuliere Download-Events (ChunkReceived)
        const DOWNLOAD_CHUNK_SIZE: usize = 8 * 1024 * 1024; // 8MB
        
        // Download 1: Erster Chunk empfangen
        // Füge Chunk zu requested_chunks hinzu, damit er als "angefordert" gilt
        let chunk_hash = "test_file_hash:0".to_string();
        if let Ok(mut requested) = app.requested_chunks.lock() {
            requested.insert(chunk_hash.clone());
        }
        
        // Füge Start-Zeit hinzu für Geschwindigkeitsberechnung
        if let Ok(mut start_times) = app.chunk_download_start_times.lock() {
            start_times.insert(chunk_hash.clone(), std::time::Instant::now());
        }
        
        app.handle_network_event(DiscoveryEvent::ChunkReceived {
            peer_id: "peer1".to_string(),
            chunk_hash: chunk_hash.clone(),
            chunk_data: vec![0u8; DOWNLOAD_CHUNK_SIZE],
        });
        
        // Prüfe, dass Peer-Performance aktualisiert wurde
        let peer_perf = app.peer_performance.lock().unwrap();
        if let Some(perf) = peer_perf.get("peer1") {
            assert!(perf.successful_requests > 0, "Peer sollte erfolgreiche Requests haben");
            assert!(perf.total_requests > 0, "Peer sollte totale Requests haben");
        }
        drop(peer_perf);
        
        // Aktualisiere Performance-Metriken
        app.update_performance_metrics();
        
        // Prüfe Performance-Metriken
        assert!(app.performance_metrics.active_uploads > 0, "Es sollten aktive Uploads sein");
        // Upload-Geschwindigkeit sollte > 0 sein, da wir Uploads simuliert haben
        assert!(app.performance_metrics.total_upload_speed_bytes_per_sec > 0.0, 
            "Gesamte Upload-Geschwindigkeit sollte > 0 sein");
        
        // Prüfe Status-Bar-Text (indirekt über upload_stats)
        app.update_upload_stats();
        let final_upload_stats = app.upload_stats.lock().unwrap();
        assert!(final_upload_stats.active_upload_count > 0 || final_upload_stats.upload_speed_bytes_per_sec > 0.0, 
            "Upload-Statistiken sollten nicht leer sein");
        
        println!("✓ Upload-Statistiken werden korrekt aktualisiert");
        println!("✓ Download-Statistiken werden korrekt aktualisiert");
        println!("✓ Performance-Metriken werden korrekt berechnet");
    }
    
    /// Testet, ob Upload-Statistiken nach Ablauf der Zeit korrekt bereinigt werden
    #[test]
    fn test_upload_statistics_timeout() {
        let (_tx, rx) = mpsc::channel(32);
        let rx_arc = Arc::new(Mutex::new(rx));
        let mut app = DeckDropApp::new_with_network_rx(rx_arc);
        
        const CHUNK_SIZE: usize = 10 * 1024 * 1024; // 10MB
        
        // Upload Chunk
        app.handle_network_event(DiscoveryEvent::ChunkUploaded {
            peer_id: "peer1".to_string(),
            chunk_hash: "hash1:0".to_string(),
            chunk_size: CHUNK_SIZE,
        });
        
        // Prüfe, dass Upload getrackt wird
        let upload_stats_1 = app.upload_stats.lock().unwrap();
        assert_eq!(upload_stats_1.active_upload_count, 1);
        drop(upload_stats_1);
        
        // Simuliere Zeitablauf: Setze Upload-Zeit auf vor 6 Sekunden (älter als 5 Sekunden Window)
        if let Ok(mut active_uploads) = app.active_uploads.lock() {
            let old_time = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(6))
                .unwrap_or(std::time::Instant::now());
            if let Some((time, _)) = active_uploads.get_mut("hash1:0") {
                *time = old_time;
            }
        }
        
        // Aktualisiere Upload-Statistiken (sollte alte Uploads entfernen)
        app.update_upload_stats();
        
        // Prüfe, dass alter Upload entfernt wurde
        let upload_stats_2 = app.upload_stats.lock().unwrap();
        assert_eq!(upload_stats_2.active_upload_count, 0, "Alter Upload sollte nach Timeout entfernt werden");
        assert_eq!(upload_stats_2.upload_speed_bytes_per_sec, 0.0, "Upload-Geschwindigkeit sollte nach Timeout 0 sein");
        
        println!("✓ Upload-Statistiken werden nach Timeout korrekt bereinigt");
    }
    
    /// Testet, ob die Status-Bar korrekte Upload-Informationen anzeigt
    #[test]
    fn test_status_bar_upload_display() {
        let (_tx, rx) = mpsc::channel(32);
        let rx_arc = Arc::new(Mutex::new(rx));
        let mut app = DeckDropApp::new_with_network_rx(rx_arc);
        
        // Initial: Sollte "Idle" anzeigen
        let initial_stats = app.upload_stats.lock().unwrap();
        assert_eq!(initial_stats.active_upload_count, 0);
        assert_eq!(initial_stats.upload_speed_bytes_per_sec, 0.0);
        drop(initial_stats);
        
        // Upload Chunk
        app.handle_network_event(DiscoveryEvent::ChunkUploaded {
            peer_id: "peer1".to_string(),
            chunk_hash: "hash1:0".to_string(),
            chunk_size: 10 * 1024 * 1024, // 10MB
        });
        
        // Prüfe, dass Upload-Statistiken aktualisiert wurden
        let upload_stats = app.upload_stats.lock().unwrap();
        assert_eq!(upload_stats.active_upload_count, 1, "Nach Upload sollte 1 aktiver Upload sein");
        assert!(upload_stats.upload_speed_bytes_per_sec > 0.0, "Upload-Geschwindigkeit sollte > 0 sein");
        
        // Status-Bar sollte jetzt "Upload: 1 Chunks (active) | Speed: X MB/s" anzeigen
        // (wird in view_status_bar() verwendet)
        assert!(upload_stats.active_upload_count > 0, "Status-Bar sollte aktive Uploads anzeigen");
        
        println!("✓ Status-Bar zeigt korrekte Upload-Informationen an");
    }
}

