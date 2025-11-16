//! Main app structure for Iced

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, progress_bar, Column, Row, Space},
    Alignment, Element, Length, Theme, Color, Task,
};
use toml;
use deckdrop_core::{Config, GameInfo, DownloadManifest, DownloadStatus};
use deckdrop_network::network::discovery::DiscoveryEvent;
use deckdrop_network::network::games::NetworkGameInfo;
use deckdrop_network::network::peer::PeerInfo;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Game integrity status
#[derive(Debug, Clone, PartialEq)]
pub enum GameIntegrityStatus {
    Checking { current: usize, total: usize },
    Intact,
    Changed,
    Error(String),
}

/// Main application state
#[derive(Debug, Clone)]
pub struct DeckDropApp {
    // Tabs
    pub current_tab: Tab,
    
    // Daten
    pub my_games: Vec<(PathBuf, GameInfo)>,
    pub game_integrity_status: HashMap<PathBuf, GameIntegrityStatus>, // game_path -> status
    pub integrity_check_start_time: HashMap<PathBuf, std::time::Instant>, // game_path -> start time for progress tracking
    pub integrity_check_progress: Arc<std::sync::Mutex<HashMap<PathBuf, usize>>>, // game_path -> current progress (for real-time updates)
    pub network_games: HashMap<String, Vec<(String, NetworkGameInfo)>>, // game_id -> [(peer_id, game_info)]
    pub peers: Vec<PeerInfo>,
    
    // Downloads
    pub active_downloads: HashMap<String, DownloadState>, // game_id -> DownloadState
    
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
    
    // Settings-Felder
    pub settings_player_name: String,
    pub settings_download_path: String,
    
    // License Dialog fields
    pub license_player_name: String,
}

/// Tab selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    MyGames,
    NetworkGames,
    Peers,
    Settings,
}

/// Download status for UI
#[derive(Debug, Clone)]
pub struct DownloadState {
    pub manifest: DownloadManifest,
    pub progress_percent: f32,
}

/// Status information
#[derive(Debug, Clone)]
pub struct StatusInfo {
    pub is_online: bool,
    pub peer_count: usize,
    pub active_download_count: usize,
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
        let mut my_games = Vec::new();
        
        // 1. Lade Spiele aus dem Download-Pfad (Unterordner = Spiele)
        if config.download_path.exists() {
            println!("[DEBUG] Loading games from download_path: {}", config.download_path.display());
            my_games.extend(deckdrop_core::load_games_from_directory(&config.download_path));
            println!("[DEBUG] Loaded {} games from download_path", my_games.len());
        } else {
            println!("[DEBUG] Download path does not exist: {}", config.download_path.display());
        }
        
        // 2. Lade Spiele aus den manuell hinzugefügten Pfaden (game_paths)
        for game_path in &config.game_paths {
            println!("[DEBUG] Loading games from game_path: {}", game_path.display());
            // Check if the path itself is a game (has deckdrop.toml)
            if deckdrop_core::check_game_config_exists(game_path) {
                if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(game_path) {
                    my_games.push((game_path.clone(), game_info));
                    println!("[DEBUG] Added game from path: {}", game_path.display());
                }
            }
            // Also check subdirectories
            let before_count = my_games.len();
            my_games.extend(deckdrop_core::load_games_from_directory(game_path));
            println!("[DEBUG] Added {} games from subdirectories of {}", 
                my_games.len() - before_count, game_path.display());
        }
        
        // 3. Lade aktive Downloads aus Manifesten und füge sie zu my_games hinzu
        let active_downloads_from_manifests = deckdrop_core::load_active_downloads();
        let mut active_downloads = HashMap::new();
        println!("[DEBUG] Loading {} active downloads from manifests", active_downloads_from_manifests.len());
        for (game_id, manifest) in active_downloads_from_manifests {
            // Versuche, die GameInfo aus dem Manifest-Verzeichnis zu laden
            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                if let Some(manifest_dir) = manifest_path.parent() {
                    if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(manifest_dir) {
                        // Füge das Spiel nur hinzu, wenn es nicht bereits in my_games ist
                        let game_path = PathBuf::from(&manifest.game_path);
                        if !my_games.iter().any(|(path, _)| path == &game_path) {
                            my_games.push((game_path.clone(), game_info));
                            println!("[DEBUG] Added download game: {} (ID: {})", manifest.game_name, game_id);
                        }
                    }
                }
            }
            // Initialisiere DownloadState für UI
            let progress_percent = manifest.progress.percentage as f32;
            active_downloads.insert(game_id.clone(), DownloadState {
                manifest: manifest.clone(),
                progress_percent,
            });
        }
        
        println!("[DEBUG] Total games loaded: {} (including {} downloads)", my_games.len(), active_downloads.len());
        
        // Deduplicate games by game_id before initializing integrity status
        {
            use std::collections::HashMap;
            let mut seen_ids = HashMap::new();
            let mut deduplicated = Vec::new();
            
            for (game_path, game_info) in &my_games {
                let game_id = &game_info.game_id;
                if !seen_ids.contains_key(game_id) {
                    seen_ids.insert(game_id.clone(), game_path.clone());
                    deduplicated.push((game_path.clone(), game_info.clone()));
                } else {
                    println!("[DEBUG] Duplicate game_id detected in Default::default(): {} (path: {}), keeping first occurrence", 
                        game_id, game_path.display());
                }
            }
            
            my_games = deduplicated;
        }
        
        let mut game_integrity_status = HashMap::new();
        // Initialize all games as "Checking" - get total file count immediately
        println!("[DEBUG] Default::default(): Initializing integrity status for {} games", my_games.len());
        for (game_path, _) in &my_games {
            let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
            let total = if chunks_toml_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&chunks_toml_path) {
                    if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                        if let Some(files) = parsed.get("file").and_then(|f| f.as_array()) {
                            files.len()
                        } else {
                            println!("[DEBUG] No 'file' array found in chunks.toml for {}", game_path.display());
                            0
                        }
                    } else {
                        println!("[DEBUG] Failed to parse chunks.toml for {}", game_path.display());
                        0
                    }
                } else {
                    println!("[DEBUG] Failed to read chunks.toml for {}", game_path.display());
                    0
                }
            } else {
                println!("[DEBUG] chunks.toml does not exist for {}", game_path.display());
                0
            };
            println!("[DEBUG] Default::default(): Initialized game {}: total={}", game_path.display(), total);
            game_integrity_status.insert(game_path.clone(), GameIntegrityStatus::Checking { current: 0, total });
        }
        println!("[DEBUG] Default::default(): Initialized {} games with integrity status", game_integrity_status.len());
        
        Self {
            current_tab: Tab::MyGames,
            my_games,
            network_games: HashMap::new(),
            peers: Vec::new(),
            active_downloads: HashMap::new(),
            game_integrity_status,
            integrity_check_start_time: HashMap::new(),
            integrity_check_progress: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
            settings_player_name: config.player_name.clone(),
            settings_download_path: config.download_path.to_string_lossy().to_string(),
            license_player_name: config.player_name.clone(),
        }
    }
}

/// Messages for the application
#[derive(Debug, Clone)]
pub enum Message {
    // Tab-Navigation
    TabChanged(Tab),
    
    // Network Events
    NetworkEvent(DiscoveryEvent),
    
    // Downloads
    DownloadGame(String), // game_id
    PauseDownload(String), // game_id
    ResumeDownload(String), // game_id
    CancelDownload(String), // game_id
    
    // My Games
    AddGame,
    AddGamePathChanged(String),
    BrowseGamePath,
    AddGameNameChanged(String),
    AddGameVersionChanged(String),
    AddGameStartFileChanged(String),
    AddGameStartArgsChanged(String),
    AddGameDescriptionChanged(String),
    AddGameAdditionalInstructionsChanged(String),
    SaveGame,
    CancelAddGame,
    
    // Settings
    OpenSettings,
    SettingsPlayerNameChanged(String),
    SettingsDownloadPathChanged(String),
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
    UpdateIntegrityProgress(PathBuf, usize), // game_path, current
}

impl DeckDropApp {
    pub fn new() -> Self {
        Self::default()
    }
    
    fn new_with_network_rx(network_event_rx: Arc<std::sync::Mutex<mpsc::Receiver<DiscoveryEvent>>>) -> Self {
        let config = Config::load();
        let mut my_games = Vec::new();
        
        // 1. Lade Spiele aus dem Download-Pfad (Unterordner = Spiele)
        if config.download_path.exists() {
            my_games.extend(deckdrop_core::load_games_from_directory(&config.download_path));
        }
        
        // 2. Lade Spiele aus den manuell hinzugefügten Pfaden (game_paths)
        for game_path in &config.game_paths {
            // Check if the path itself is a game (has deckdrop.toml)
            if deckdrop_core::check_game_config_exists(game_path) {
                if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(game_path) {
                    my_games.push((game_path.clone(), game_info));
                }
            }
            // Also check subdirectories
            my_games.extend(deckdrop_core::load_games_from_directory(game_path));
        }
        
        // 3. Lade aktive Downloads aus Manifesten und füge sie zu my_games hinzu
        let active_downloads_from_manifests = deckdrop_core::load_active_downloads();
        let mut active_downloads = HashMap::new();
        for (game_id, manifest) in active_downloads_from_manifests {
            // Versuche, die GameInfo aus dem Manifest-Verzeichnis zu laden
            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                if let Some(manifest_dir) = manifest_path.parent() {
                    if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(manifest_dir) {
                        // Füge das Spiel nur hinzu, wenn es nicht bereits in my_games ist
                        let game_path = PathBuf::from(&manifest.game_path);
                        if !my_games.iter().any(|(path, _)| path == &game_path) {
                            my_games.push((game_path.clone(), game_info));
                        }
                    }
                }
            }
            // Initialisiere DownloadState für UI
            let progress_percent = manifest.progress.percentage as f32;
            active_downloads.insert(game_id.clone(), DownloadState {
                manifest: manifest.clone(),
                progress_percent,
            });
        }
        
        // Deduplicate games by game_id before initializing integrity status
        {
            use std::collections::HashMap;
            let mut seen_ids = HashMap::new();
            let mut deduplicated = Vec::new();
            
            for (game_path, game_info) in &my_games {
                let game_id = &game_info.game_id;
                if !seen_ids.contains_key(game_id) {
                    seen_ids.insert(game_id.clone(), game_path.clone());
                    deduplicated.push((game_path.clone(), game_info.clone()));
                } else {
                    println!("[DEBUG] Duplicate game_id detected in new_with_network_rx(): {} (path: {}), keeping first occurrence", 
                        game_id, game_path.display());
                }
            }
            
            my_games = deduplicated;
        }
        
        let mut game_integrity_status = HashMap::new();
        // Initialize all games as "Checking" - get total file count immediately
        println!("[DEBUG] new_with_network_rx: Initializing integrity status for {} games", my_games.len());
        for (game_path, _) in &my_games {
            let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
            let total = if chunks_toml_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&chunks_toml_path) {
                    if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                        if let Some(files) = parsed.get("file").and_then(|f| f.as_array()) {
                            files.len()
                        } else {
                            println!("[DEBUG] No 'file' array found in chunks.toml for {}", game_path.display());
                            0
                        }
                    } else {
                        println!("[DEBUG] Failed to parse chunks.toml for {}", game_path.display());
                        0
                    }
                } else {
                    println!("[DEBUG] Failed to read chunks.toml for {}", game_path.display());
                    0
                }
            } else {
                println!("[DEBUG] chunks.toml does not exist for {}", game_path.display());
                0
            };
            println!("[DEBUG] Initialized game {}: total={}", game_path.display(), total);
            game_integrity_status.insert(game_path.clone(), GameIntegrityStatus::Checking { current: 0, total });
        }
        println!("[DEBUG] new_with_network_rx: Initialized {} games with integrity status", game_integrity_status.len());
        
        Self {
            current_tab: Tab::MyGames,
            my_games,
            network_games: HashMap::new(),
            peers: Vec::new(),
            active_downloads,
            game_integrity_status,
            integrity_check_start_time: HashMap::new(),
            integrity_check_progress: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
            settings_player_name: config.player_name.clone(),
            settings_download_path: config.download_path.to_string_lossy().to_string(),
            license_player_name: config.player_name.clone(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabChanged(tab) => {
                self.current_tab = tab;
                // Initialize settings fields when Settings tab is opened
                if tab == Tab::Settings {
                    self.settings_player_name = self.config.player_name.clone();
                    self.settings_download_path = self.config.download_path.to_string_lossy().to_string();
                }
            }
            Message::GameIntegrityChecked(game_path, status) => {
                println!("[DEBUG] GameIntegrityChecked: {} -> {:?}", game_path.display(), status);
                self.game_integrity_status.insert(game_path.clone(), status);
                // Remove start time when check is complete
                self.integrity_check_start_time.remove(&game_path);
                println!("[DEBUG] Status updated and start time removed");
            }
            Message::UpdateIntegrityProgress(game_path, current) => {
                println!("[DEBUG] UpdateIntegrityProgress: {} -> current={}", game_path.display(), current);
                // Update progress for a checking game
                if let Some(status) = self.game_integrity_status.get_mut(&game_path) {
                    if let GameIntegrityStatus::Checking { total, .. } = status {
                        println!("[DEBUG] Updating status: {} -> Checking {{ current: {}, total: {} }}", 
                            game_path.display(), current, total);
                        *status = GameIntegrityStatus::Checking { current, total: *total };
                        // Return a task to continue checking for more updates
                        // This ensures the UI keeps updating
                        return Task::perform(
                            async move {
                                use futures_timer::Delay;
                                use std::time::Duration;
                                Delay::new(Duration::from_millis(50)).await;
                                Message::Tick
                            },
                            |msg| msg
                        );
                    } else {
                        println!("[DEBUG] Status is not Checking, ignoring update");
                    }
                } else {
                    println!("[DEBUG] Game path not found in integrity_status");
                }
            }
            Message::NetworkEvent(event) => {
                self.handle_network_event(event);
            }
            Message::NetworkEventReceived(event) => {
                self.handle_network_event(event);
            }
            Message::DownloadGame(game_id) => {
                // Find peer for this download
                if let Some(peers) = self.network_games.get(&game_id) {
                    if let Some((peer_id, _)) = peers.first() {
                        // Start download via Network-Bridge
                        if let Some(tx) = crate::network_bridge::get_download_request_tx() {
                            let _ = tx.send(deckdrop_network::network::discovery::DownloadRequest::RequestGameMetadata {
                                peer_id: peer_id.clone(),
                                game_id: game_id.clone(),
                            });
                        }
                    }
                }
            }
            Message::PauseDownload(game_id) => {
                // Pause download
                if let Some(download_state) = self.active_downloads.get_mut(&game_id) {
                    // TODO: Implement pause functionality in synch.rs
                    // For now: Set status to Paused
                    download_state.manifest.overall_status = deckdrop_core::DownloadStatus::Paused;
                }
            }
            Message::ResumeDownload(game_id) => {
                // Resume download
                if let Some(download_state) = self.active_downloads.get_mut(&game_id) {
                    // TODO: Implement resume functionality in synch.rs
                    // For now: Set status to Downloading
                    download_state.manifest.overall_status = deckdrop_core::DownloadStatus::Downloading;
                }
            }
            Message::CancelDownload(game_id) => {
                // Cancel download
                if let Some(peers) = self.network_games.get(&game_id) {
                    if let Some((peer_id, _)) = peers.first() {
                        // Cancel download (local)
                        if let Err(e) = deckdrop_core::cancel_game_download(&game_id) {
                            eprintln!("Error canceling download for {}: {}", game_id, e);
                        }
                    }
                }
                // Remove from active downloads
                self.active_downloads.remove(&game_id);
            }
            Message::AddGame => {
                self.show_add_game_dialog = true;
            }
            Message::AddGamePathChanged(path) => {
                self.add_game_path = path;
            }
            Message::BrowseGamePath => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Select Game Directory")
                    .pick_folder()
                {
                    self.add_game_path = path.to_string_lossy().to_string();
                }
            }
            Message::AddGameNameChanged(name) => {
                self.add_game_name = name;
            }
            Message::AddGameVersionChanged(version) => {
                self.add_game_version = version;
            }
            Message::AddGameStartFileChanged(start_file) => {
                self.add_game_start_file = start_file;
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
                // Validate required fields
                if self.add_game_path.is_empty() || self.add_game_name.is_empty() || self.add_game_start_file.is_empty() {
                    eprintln!("Error: Path, name, and start file are required");
                    return Task::none();
                }
                
                let game_path = PathBuf::from(&self.add_game_path);
                if !game_path.exists() {
                    eprintln!("Error: Game path does not exist: {}", game_path.display());
                    return Task::none();
                }
                
                // Create GameInfo
                let mut game_info = GameInfo {
                    game_id: deckdrop_core::game::generate_game_id(),
                    name: self.add_game_name.clone(),
                    version: if self.add_game_version.is_empty() { "1.0".to_string() } else { self.add_game_version.clone() },
                    start_file: self.add_game_start_file.clone(),
                    start_args: if self.add_game_start_args.is_empty() { None } else { Some(self.add_game_start_args.clone()) },
                    description: if self.add_game_description.is_empty() { None } else { Some(self.add_game_description.clone()) },
                    additional_instructions: if self.add_game_additional_instructions.is_empty() { None } else { Some(self.add_game_additional_instructions.clone()) },
                    creator_peer_id: self.config.peer_id.clone(),
                    hash: None,
                };
                
                // Generate chunks.toml
                if let Err(e) = deckdrop_core::generate_chunks_toml(&game_path, None::<fn(usize, usize, &str)>) {
                    eprintln!("Error generating chunks.toml: {}", e);
                    return Task::none();
                }
                
                // Load chunks.toml to get hash
                let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                if chunks_toml_path.exists() {
                    if let Ok(hash) = deckdrop_core::gamechecker::calculate_file_hash(&chunks_toml_path) {
                        game_info.hash = Some(format!("blake3:{}", hash));
                    }
                }
                
                // Save GameInfo
                if let Err(e) = game_info.save_to_path_with_hash(&game_path, game_info.hash.clone()) {
                    eprintln!("Error saving game: {}", e);
                    return Task::none();
                }
                
                // Add game path to config
                let mut config = deckdrop_core::Config::load();
                if let Err(e) = config.add_game_path(&game_path) {
                    eprintln!("Error adding game path to config: {}", e);
                }
                
                // Reload games list
                self.config = config.clone();
                self.my_games.clear();
                self.game_integrity_status.clear();
                
                // 1. Lade Spiele aus dem Download-Pfad (Unterordner = Spiele)
                if self.config.download_path.exists() {
                    self.my_games.extend(deckdrop_core::load_games_from_directory(&self.config.download_path));
                }
                
                // 2. Lade Spiele aus den manuell hinzugefügten Pfaden (game_paths)
                for game_path_dir in &self.config.game_paths {
                    // Check if the path itself is a game (has deckdrop.toml)
                    if deckdrop_core::check_game_config_exists(game_path_dir) {
                        if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(game_path_dir) {
                            self.my_games.push((game_path_dir.clone(), game_info));
                        }
                    }
                    // Also check subdirectories
                    self.my_games.extend(deckdrop_core::load_games_from_directory(game_path_dir));
                }
                
                // Deduplicate games by game_id
                self.deduplicate_games_by_id();
                
                // Initialize integrity status for all games as "Checking" - get total file count immediately
                for (game_path, _) in &self.my_games {
                    let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                    let total = if chunks_toml_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&chunks_toml_path) {
                            if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                                if let Some(files) = parsed.get("file").and_then(|f| f.as_array()) {
                                    files.len()
                                } else {
                                    0
                                }
                            } else {
                                0
                            }
                        } else {
                            0
                        }
                    } else {
                        0
                    };
                    self.game_integrity_status.insert(game_path.clone(), GameIntegrityStatus::Checking { current: 0, total });
                    // Don't set start time here - it will be set when the check actually starts
                }
                
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
                if let Some(tx) = crate::network_bridge::get_metadata_update_tx() {
                    let games_count = self.my_games.len() as u32;
                    let _ = tx.send(deckdrop_network::network::discovery::MetadataUpdate {
                        player_name: None, // Only update games count
                        games_count: Some(games_count),
                    });
                }
            }
            Message::CancelAddGame => {
                self.show_add_game_dialog = false;
            }
            Message::OpenSettings => {
                self.show_settings = true;
                self.settings_player_name = self.config.player_name.clone();
                self.settings_download_path = self.config.download_path.to_string_lossy().to_string();
            }
            Message::SettingsPlayerNameChanged(name) => {
                self.settings_player_name = name;
            }
            Message::SettingsDownloadPathChanged(path) => {
                self.settings_download_path = path;
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
                if let Err(e) = self.config.save() {
                    eprintln!("Error saving config: {}", e);
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
                println!("[DEBUG] ===== TICK RECEIVED =====");
                println!("[DEBUG] Total games in integrity_status: {}", self.game_integrity_status.len());
                println!("[DEBUG] Total games in my_games: {}", self.my_games.len());
                
                // Periodic updates (e.g., update download progress)
                self.update_download_progress();
                
                // Check for Network events (non-blocking) via global access
                if let Some(rx) = crate::network_bridge::get_network_event_rx() {
                    if let Ok(mut rx) = rx.lock() {
                        while let Ok(event) = rx.try_recv() {
                            self.handle_network_event(event);
                        }
                    }
                }
                
                // FIRST: Check if we need to start an integrity check (before updating progress)
                // This ensures the check starts immediately, not after simulated progress
                println!("[DEBUG] Filtering games to check (before progress update)...");
                let games_to_check: Vec<PathBuf> = self.game_integrity_status
                    .iter()
                    .filter(|(path, status)| {
                        if let GameIntegrityStatus::Checking { current, total } = status {
                            let should_check = *total > 0 && *current == 0;
                            println!("[DEBUG] Game {}: current={}, total={}, should_check={}", 
                                path.display(), current, total, should_check);
                            should_check
                        } else {
                            println!("[DEBUG] Game {}: status is not Checking", path.display());
                            false
                        }
                    })
                    .map(|(path, _)| path.clone())
                    .collect();
                
                println!("[DEBUG] Games to check: {} (games with current=0 and total>0)", games_to_check.len());
                for game_path in &games_to_check {
                    println!("[DEBUG]   - {}", game_path.display());
                }
                
                // If we have games to check, start the check immediately
                if !games_to_check.is_empty() {
                    // Start integrity check for first game that needs checking
                    let game_path = games_to_check[0].clone();
                    println!("[DEBUG] Starting integrity check for: {}", game_path.display());
                    
                    // Get the total file count (should already be set during initialization)
                    let total = if let Some(GameIntegrityStatus::Checking { total, .. }) = self.game_integrity_status.get(&game_path) {
                        *total
                    } else {
                        0
                    };
                    
                    println!("[DEBUG] Total files to check: {}", total);
                    
                    // Set start time for progress tracking
                    self.integrity_check_start_time.insert(game_path.clone(), std::time::Instant::now());
                    println!("[DEBUG] Start time set for: {}", game_path.display());
                    
                    // Start the actual integrity check with progress tracking
                    let game_path_for_check = game_path.clone();
                    let progress_tracker = self.integrity_check_progress.clone();
                    return Task::perform(
                        {
                            let game_path_clone = game_path_for_check.clone();
                            let progress_tracker_clone = progress_tracker.clone();
                            async move {
                                println!("[DEBUG] Starting async integrity check for: {}", game_path_clone.display());
                                // Perform light check (fast, checks for file changes)
                                let result = deckdrop_core::light_check_game(&game_path_clone);
                                println!("[DEBUG] Light check result: {:?}", result.is_ok());
                                match result {
                                    Ok(light_result) => {
                                        println!("[DEBUG] Light check: missing_files={}, extra_files={}", 
                                            light_result.missing_files.len(), light_result.extra_files.len());
                                        if light_result.missing_files.is_empty() && light_result.extra_files.is_empty() {
                                            // All files match, do full integrity check with progress
                                            println!("[DEBUG] Starting full integrity check with progress");
                                            match deckdrop_core::verify_game_integrity_with_progress(
                                                &game_path_clone,
                                                Some(|current, total| {
                                                    // Update progress in shared tracker
                                                    if let Ok(mut progress) = progress_tracker_clone.lock() {
                                                        progress.insert(game_path_clone.clone(), current);
                                                        println!("[DEBUG] Progress update: {}/{}", current, total);
                                                    }
                                                })
                                            ) {
                                                Ok(integrity_result) => {
                                                    println!("[DEBUG] Integrity check complete: failed_files={}, missing_files={}", 
                                                        integrity_result.failed_files.len(), integrity_result.missing_files.len());
                                                    // Clear progress tracker
                                                    if let Ok(mut progress) = progress_tracker_clone.lock() {
                                                        progress.remove(&game_path_clone);
                                                    }
                                                    if integrity_result.failed_files.is_empty() && integrity_result.missing_files.is_empty() {
                                                        (game_path_clone, GameIntegrityStatus::Intact)
                                                    } else {
                                                        (game_path_clone, GameIntegrityStatus::Changed)
                                                    }
                                                }
                                                Err(e) => {
                                                    println!("[DEBUG] Integrity check error: {}", e);
                                                    // Clear progress tracker
                                                    if let Ok(mut progress) = progress_tracker_clone.lock() {
                                                        progress.remove(&game_path_clone);
                                                    }
                                                    (game_path_clone, GameIntegrityStatus::Changed)
                                                }
                                            }
                                        } else {
                                            // Clear progress tracker
                                            if let Ok(mut progress) = progress_tracker_clone.lock() {
                                                progress.remove(&game_path_clone);
                                            }
                                            (game_path_clone, GameIntegrityStatus::Changed)
                                        }
                                    }
                                    Err(e) => {
                                        println!("[DEBUG] Light check error: {}", e);
                                        // Clear progress tracker
                                        if let Ok(mut progress) = progress_tracker_clone.lock() {
                                            progress.remove(&game_path_clone);
                                        }
                                        (game_path_clone, GameIntegrityStatus::Error(e.to_string()))
                                    }
                                }
                            }
                        },
                        |(path, status)| {
                            println!("[DEBUG] Integrity check complete: {} -> {:?}", path.display(), status);
                            Message::GameIntegrityChecked(path, status)
                        }
                    );
                }
                
                // Update progress from tracker for games that are currently checking
                let mut progress_updates = Vec::new();
                if let Ok(progress) = self.integrity_check_progress.lock() {
                    for (game_path, current_progress) in progress.iter() {
                        if let Some(GameIntegrityStatus::Checking { total, .. }) = self.game_integrity_status.get(game_path) {
                            if *current_progress > 0 && *current_progress <= *total {
                                progress_updates.push((game_path.clone(), *current_progress));
                            }
                        }
                    }
                }
                
                // Send progress updates as messages to trigger UI re-render
                if !progress_updates.is_empty() {
                    println!("[DEBUG] Sending {} progress updates from tracker", progress_updates.len());
                    let (game_path, current) = progress_updates[0].clone();
                    return Task::perform(
                        async move {
                            use futures_timer::Delay;
                            use std::time::Duration;
                            Delay::new(Duration::from_millis(10)).await;
                            (game_path, current)
                        },
                        |(path, current)| Message::UpdateIntegrityProgress(path, current)
                    );
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<Message> {
        // Main layout
        let content = if self.show_license_dialog {
            self.view_license_dialog()
        } else if self.show_settings {
            self.view_settings()
        } else if self.show_add_game_dialog {
            self.view_add_game_dialog()
        } else {
            column![
                self.view_tabs(),
                self.view_current_tab(),
            ]
            .spacing(10)
            .into()
        };
        
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
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
                // Find existing peer or add new one
                if let Some(existing_peer) = self.peers.iter_mut().find(|p| p.id == peer_info.id) {
                    // Update existing peer with new information
                    existing_peer.player_name = peer_info.player_name.clone();
                    existing_peer.games_count = peer_info.games_count;
                    existing_peer.addr = peer_info.addr.clone();
                    existing_peer.version = peer_info.version.clone();
                } else {
                    // Add new peer
                    self.peers.push(peer_info);
                    self.status.peer_count = self.peers.len();
                }
            }
            DiscoveryEvent::PeerLost(peer_id) => {
                self.peers.retain(|p| p.id != peer_id);
                self.status.peer_count = self.peers.len();
                // Also remove games from this peer
                self.network_games.retain(|_, games| {
                    games.retain(|(pid, _)| pid != &peer_id);
                    !games.is_empty()
                });
            }
            DiscoveryEvent::GamesListReceived { peer_id, games } => {
                for game in games {
                    let game_id = game.game_id.clone();
                    self.network_games
                        .entry(game_id)
                        .or_insert_with(Vec::new)
                        .push((peer_id.clone(), game));
                }
            }
            DiscoveryEvent::GameMetadataReceived { peer_id: _, game_id, deckdrop_toml, deckdrop_chunks_toml } => {
                // Start download with received metadata
                if let Err(e) = deckdrop_core::start_game_download(
                    &game_id,
                    &deckdrop_toml,
                    &deckdrop_chunks_toml,
                ) {
                    eprintln!("Error starting download for {}: {}", game_id, e);
                } else {
                    // Load manifest for UI update
                    if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                        if let Ok(manifest) = deckdrop_core::DownloadManifest::load(&manifest_path) {
                            let progress_percent = manifest.progress.percentage as f32;
                            
                            self.active_downloads.insert(game_id.clone(), DownloadState {
                                manifest,
                                progress_percent,
                            });
                        }
                    }
                }
            }
            DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data: _ } => {
                // TODO: Process chunk
                println!("ChunkReceived: {} from {}", chunk_hash, peer_id);
            }
            DiscoveryEvent::ChunkRequestFailed { peer_id, chunk_hash, error } => {
                eprintln!("ChunkRequestFailed: {} from {}: {}", chunk_hash, peer_id, error);
            }
        }
    }
    
    /// Updates download progress
    fn update_download_progress(&mut self) {
        // Load all active downloads from manifests (including those not in network_games)
        let active_downloads_from_manifests = deckdrop_core::load_active_downloads();
        
        // Track which games need to be finalized (only finalize once)
        let mut games_to_finalize = Vec::new();
        
        // Update active_downloads with all manifests
        for (game_id, manifest) in active_downloads_from_manifests {
            let progress_percent = manifest.progress.percentage as f32;
            
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
            
            // Update or add download state
            self.active_downloads.insert(game_id.clone(), DownloadState {
                manifest: manifest.clone(),
                progress_percent,
            });
        }
        
        // Finalize downloads that need it (only once)
        for (game_id, manifest) in games_to_finalize {
            // Finalize the download
            if let Err(e) = deckdrop_core::finalize_game_download(&game_id, &manifest) {
                eprintln!("Error finalizing download for {}: {}", game_id, e);
            } else {
                // Reload games list to include the finalized game
                let config = deckdrop_core::Config::load();
                self.my_games.clear();
                
                // Reload games from download path
                if config.download_path.exists() {
                    self.my_games.extend(deckdrop_core::load_games_from_directory(&config.download_path));
                }
                
                // Reload games from game_paths
                for game_path in &config.game_paths {
                    if deckdrop_core::check_game_config_exists(game_path) {
                        if let Ok(game_info) = deckdrop_core::GameInfo::load_from_path(game_path) {
                            self.my_games.push((game_path.clone(), game_info));
                        }
                    }
                    self.my_games.extend(deckdrop_core::load_games_from_directory(game_path));
                }
                
                // Deduplicate games by game_id
                self.deduplicate_games_by_id();
                
                // Initialize integrity status for all games
                for (game_path, _) in &self.my_games {
                    if !self.game_integrity_status.contains_key(game_path) {
                        let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                        let total = if chunks_toml_path.exists() {
                            if let Ok(content) = std::fs::read_to_string(&chunks_toml_path) {
                                if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                                    if let Some(files) = parsed.get("file").and_then(|f| f.as_array()) {
                                        files.len()
                                    } else {
                                        0
                                    }
                                } else {
                                    0
                                }
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                        self.game_integrity_status.insert(game_path.clone(), GameIntegrityStatus::Checking { current: 0, total });
                    }
                }
            }
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
                                
                                // Initialize integrity status for downloading game (only if not already set)
                                if !self.game_integrity_status.contains_key(&game_path) {
                                    let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                                    let total = if chunks_toml_path.exists() {
                                        if let Ok(content) = std::fs::read_to_string(&chunks_toml_path) {
                                            if let Ok(parsed) = toml::from_str::<toml::Value>(&content) {
                                                if let Some(files) = parsed.get("file").and_then(|f| f.as_array()) {
                                                    files.len()
                                                } else {
                                                    0
                                                }
                                            } else {
                                                0
                                            }
                                        } else {
                                            0
                                        }
                                    } else {
                                        0
                                    };
                                    self.game_integrity_status.insert(game_path, GameIntegrityStatus::Checking { current: 0, total });
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Resume downloads if host is available
        let games_to_resume: Vec<String> = self.active_downloads
            .iter()
            .filter(|(_, download_state)| {
                matches!(download_state.manifest.overall_status, deckdrop_core::DownloadStatus::Pending | deckdrop_core::DownloadStatus::Paused)
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
                
                // Request missing chunks to resume download
                if let Some(tx) = crate::network_bridge::get_download_request_tx() {
                    if let Err(e) = deckdrop_core::request_missing_chunks(&game_id, &peer_ids, &tx, 3) {
                        eprintln!("Error resuming download for {}: {}", game_id, e);
                    } else {
                        // Update status to Downloading
                        if let Some(ds) = self.active_downloads.get_mut(&game_id) {
                            ds.manifest.overall_status = deckdrop_core::DownloadStatus::Downloading;
                            
                            // Save updated manifest
                            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(&game_id) {
                                let _ = ds.manifest.save(&manifest_path);
                            }
                        }
                        println!("Resumed download for game: {}", game_id);
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
        
        self.status.active_download_count = self.active_downloads.len();
    }
    
    /// Shows tabs
    fn view_tabs(&self) -> Element<Message> {
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
            button("Settings")
                .on_press(Message::TabChanged(Tab::Settings))
                .style(if self.current_tab == Tab::Settings {
                    button::primary
                } else {
                    button::secondary
                }),
        ]
        .spacing(10)
        .into()
    }
    
    /// Shows current tab
    fn view_current_tab(&self) -> Element<Message> {
        match self.current_tab {
            Tab::MyGames => self.view_my_games(),
            Tab::NetworkGames => self.view_network_games(),
            Tab::Peers => self.view_peers(),
            Tab::Settings => self.view_settings_tab(),
        }
    }
    
    /// Shows "My Games" tab
    fn view_my_games(&self) -> Element<Message> {
        let mut games_column = Column::new()
            .spacing(10)
            .padding(10);
        
        // Header with "Add Game" button
        games_column = games_column.push(
            row![
                text("My Games").size(24),
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
                    .unwrap_or(&GameIntegrityStatus::Checking { current: 0, total: 0 });
                
                let mut game_column = column![
                    text(&game.name).size(18),
                    text(format!("Version: {}", game.version)).size(14),
                    text(format!("Path: {}", game_path.display())).size(12),
                ];
                
                // Show download status if downloading
                if let Some(ds) = download_state {
                    let download_status_text = match ds.manifest.overall_status {
                        deckdrop_core::DownloadStatus::Downloading => {
                            format!("Downloading... {:.1}%", ds.progress_percent)
                        }
                        deckdrop_core::DownloadStatus::Paused => "Paused".to_string(),
                        deckdrop_core::DownloadStatus::Pending => "Waiting for host...".to_string(),
                        deckdrop_core::DownloadStatus::Complete => "Download complete".to_string(),
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
                            .size(11)
                            .style(move |_theme: &Theme| {
                                iced::widget::text::Style {
                                    color: Some(download_status_color),
                                }
                            })
                    );
                    
                    // Show progress bar for active downloads
                    if matches!(ds.manifest.overall_status, deckdrop_core::DownloadStatus::Downloading) {
                        game_column = game_column.push(
                            progress_bar(0.0..=100.0, ds.progress_percent)
                                .width(Length::Fill)
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
                        GameIntegrityStatus::Checking { current, total } => {
                            if *total > 0 {
                                (format!("Checking {}/{}...", *current + 1, *total), Color::from_rgba(0.7, 0.7, 0.7, 1.0))
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
                            .size(11)
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
                        .padding(15)
                );
            }
        }
        
        scrollable(games_column)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    
    /// Shows "Network Games" tab
    fn view_network_games(&self) -> Element<Message> {
        let mut games_column = Column::new()
            .spacing(10)
            .padding(10);
        
        games_column = games_column.push(
            text("Network Games").size(24)
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
                    
                    let mut game_column = column![
                        row![
                            column![
                                text(&game_info.name).size(18),
                                text(format!("Version: {}", game_info.version)).size(14),
                                text(format!("From: {} Peer(s)", games.len())).size(12),
                            ]
                            .width(Length::Fill),
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
                                            .on_press(Message::PauseDownload(game_id.clone()))
                                            .style(button::secondary)
                                    } else if can_resume {
                                        button("Resume")
                                            .on_press(Message::ResumeDownload(game_id.clone()))
                                            .style(button::secondary)
                                    } else {
                                        button("Downloading...")
                                            .style(button::secondary)
                                    },
                                    button("Cancel")
                                        .on_press(Message::CancelDownload(game_id.clone())),
                                ]
                                .spacing(5)
                            } else {
                                column![
                                    button("Get this game")
                                        .on_press(Message::DownloadGame(game_id.clone()))
                                        .style(button::primary),
                                ]
                            },
                        ]
                        .spacing(10),
                    ];
                    
                    // Show progress bar when download is active
                    if let Some(download_state) = download_state {
                        game_column = game_column.push(
                            column![
                                text(format!("Progress: {:.1}%", download_state.progress_percent)).size(12),
                                progress_bar(0.0..=100.0, download_state.progress_percent)
                                    .width(Length::Fill),
                            ]
                            .spacing(5)
                        );
                        
                        // Show status
                        let status_text = match download_state.manifest.overall_status {
                            deckdrop_core::DownloadStatus::Downloading => "Downloading...",
                            deckdrop_core::DownloadStatus::Paused => "Paused",
                            deckdrop_core::DownloadStatus::Complete => "Completed",
                            deckdrop_core::DownloadStatus::Error(_) => "Failed",
                            deckdrop_core::DownloadStatus::Pending => "Pending",
                            deckdrop_core::DownloadStatus::Cancelled => "Cancelled",
                        };
                        game_column = game_column.push(
                            text(status_text).size(12)
                        );
                    }
                    
                    games_column = games_column.push(
                        container(game_column)
                            .style(container_box_style)
                            .width(Length::Fill)
                            .padding(15)
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
    fn view_peers(&self) -> Element<Message> {
        let mut peers_column = Column::new()
            .spacing(10)
            .padding(10);
        
        peers_column = peers_column.push(
            text("Found Peers").size(24)
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
                            text(format!("Player: {}", player_name)).size(16),
                            text(format!("Peer ID: {}", &peer.id[..16.min(peer.id.len())])).size(12),
                            text(format!("Games: {}", games_count)).size(12),
                            text(format!("Version: {}", version)).size(12),
                        ]
                        .spacing(5)
                        .padding(15)
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
    fn view_settings_tab(&self) -> Element<Message> {
        let version = env!("CARGO_PKG_VERSION");
        column![
            text("Settings").size(24),
            text_input("Player Name", &self.settings_player_name)
                .on_input(Message::SettingsPlayerNameChanged)
                .padding(10),
            row![
                text_input("Download Path", &self.config.download_path.to_string_lossy())
                    .on_input(Message::SettingsDownloadPathChanged)
                    .padding(10),
                button("Browse...")
                    .on_press(Message::BrowseDownloadPath)
                    .padding(10),
            ]
            .spacing(10),
            text(format!("Version: {}", version)).size(12),
            row![
                button("Save")
                    .on_press(Message::SaveSettings),
            ]
            .spacing(10),
        ]
        .spacing(15)
        .padding(20)
        .into()
    }
    
    /// Shows license dialog
    fn view_license_dialog(&self) -> Element<Message> {
        container(
            column![
                text("Welcome to DeckDrop").size(28),
                Space::with_height(20),
                text("Before you can use DeckDrop, you must agree to the terms and conditions.").size(16),
                Space::with_height(20),
                text("Player Name:").size(16),
                text_input("Enter your player name", &self.license_player_name)
                    .on_input(Message::LicensePlayerNameChanged)
                    .padding(10),
                Space::with_height(20),
                scrollable(
                    text("DeckDrop is a peer-to-peer game sharing platform.\n\n\
                          By using DeckDrop, you agree to:\n\n\
                          • Only share games for which you have the rights\n\
                          • Not share illegal content\n\
                          • Take responsibility for your shared content\n\n\
                          DeckDrop assumes no liability for shared content.")
                        .size(14)
                )
                .height(Length::Fixed(200.0)),
                Space::with_height(20),
                button("Accept")
                    .on_press(Message::AcceptLicense)
                    .style(button::primary),
            ]
            .spacing(15)
            .padding(30)
        )
        .width(Length::Fixed(600.0))
        .height(Length::Shrink)
        .style(container_box_style)
        .into()
    }
    
    /// Shows settings dialog
    fn view_settings(&self) -> Element<Message> {
        container(
            column![
                text("Settings").size(28),
                Space::with_height(20),
                text("Player Name:").size(16),
                text_input("Player Name", &self.settings_player_name)
                    .on_input(Message::SettingsPlayerNameChanged)
                    .padding(10),
                Space::with_height(10),
                text("Download Path:").size(16),
                row![
                    text_input("Download Path", &self.settings_download_path)
                        .on_input(Message::SettingsDownloadPathChanged)
                        .padding(10),
                    button("Browse...")
                        .on_press(Message::BrowseDownloadPath)
                        .padding(10),
                ]
                .spacing(10),
                Space::with_height(20),
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
            .spacing(15)
            .padding(30)
        )
        .width(Length::Fixed(500.0))
        .height(Length::Shrink)
        .style(container_box_style)
        .into()
    }
    
    /// Shows "Add Game" dialog
    fn view_add_game_dialog(&self) -> Element<Message> {
        container(
            scrollable(
                column![
                    text("Add Game").size(28),
                    Space::with_height(20),
                    row![
                        // Left column
                        column![
                            text("Path:").size(16),
                            row![
                                text_input("Path", &self.add_game_path)
                                    .on_input(Message::AddGamePathChanged)
                                    .padding(10),
                                button("Browse...")
                                    .on_press(Message::BrowseGamePath)
                                    .padding(10),
                            ]
                            .spacing(10),
                            Space::with_height(10),
                            text("Name:").size(16),
                            text_input("Name", &self.add_game_name)
                                .on_input(Message::AddGameNameChanged)
                                .padding(10),
                            Space::with_height(10),
                            text("Version:").size(16),
                            text_input("Version (default: 1.0)", &self.add_game_version)
                                .on_input(Message::AddGameVersionChanged)
                                .padding(10),
                            Space::with_height(10),
                            text("Start Args (optional):").size(16),
                            text_input("Start Args", &self.add_game_start_args)
                                .on_input(Message::AddGameStartArgsChanged)
                                .padding(10),
                        ]
                        .spacing(10)
                        .width(Length::Fill),
                        Space::with_width(20),
                        // Right column
                        column![
                            text("Game Executable:").size(16),
                            text_input("Relative to the game path", &self.add_game_start_file)
                                .on_input(Message::AddGameStartFileChanged)
                                .padding(10),
                            Space::with_height(10),
                            text("Description (optional):").size(16),
                            text_input("Description", &self.add_game_description)
                                .on_input(Message::AddGameDescriptionChanged)
                                .padding(10),
                            Space::with_height(10),
                            text("Additional Instructions (optional):").size(16),
                            text_input("Additional Instructions", &self.add_game_additional_instructions)
                                .on_input(Message::AddGameAdditionalInstructionsChanged)
                                .padding(10),
                        ]
                        .spacing(10)
                        .width(Length::Fill),
                    ]
                    .spacing(20)
                    .width(Length::Fill),
                    Space::with_height(20),
                    row![
                        button("Cancel")
                            .on_press(Message::CancelAddGame),
                        Space::with_width(Length::Fill),
                        button("Save")
                            .on_press(Message::SaveGame)
                            .style(button::primary),
                    ]
                    .width(Length::Fill),
                ]
                .spacing(15)
                .padding(30)
            )
            .width(Length::Fill)
            .height(Length::Fill)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(container_box_style)
        .into()
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

