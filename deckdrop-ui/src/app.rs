//! Main app structure for Iced

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, progress_bar, Column, Row, Space},
    Alignment, Element, Length, Theme, Color, Task,
};
use deckdrop_core::{Config, GameInfo, DownloadManifest, DownloadStatus};
use deckdrop_network::network::discovery::DiscoveryEvent;
use deckdrop_network::network::games::NetworkGameInfo;
use deckdrop_network::network::peer::PeerInfo;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Main application state
#[derive(Debug, Clone)]
pub struct DeckDropApp {
    // Tabs
    pub current_tab: Tab,
    
    // Daten
    pub my_games: Vec<(PathBuf, GameInfo)>,
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
    pub add_game_installation_instructions: String,
    
    // Settings-Felder
    pub settings_player_name: String,
    pub settings_download_path: String,
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
        // Create a dummy receiver for Default
        // In main() this will be replaced by the real receiver
        let (_tx, rx) = mpsc::channel(1);
        let config = Config::load();
        let mut my_games = Vec::new();
        for game_path in &config.game_paths {
            my_games.extend(deckdrop_core::load_games_from_directory(game_path));
        }
        
        Self {
            current_tab: Tab::MyGames,
            my_games,
            network_games: HashMap::new(),
            peers: Vec::new(),
            active_downloads: HashMap::new(),
            config: config.clone(),
            status: StatusInfo {
                is_online: true,
                peer_count: 0,
                active_download_count: 0,
            },
            _network_event_rx: Arc::new(std::sync::Mutex::new(rx)),
            show_license_dialog: !Config::has_peer_id(),
            show_settings: false,
            show_add_game_dialog: false,
            add_game_path: String::new(),
            add_game_name: String::new(),
            add_game_version: String::new(),
            add_game_start_file: String::new(),
            add_game_start_args: String::new(),
            add_game_description: String::new(),
            add_game_installation_instructions: String::new(),
            settings_player_name: config.player_name.clone(),
            settings_download_path: config.download_path.to_string_lossy().to_string(),
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
    AddGameNameChanged(String),
    AddGameVersionChanged(String),
    AddGameStartFileChanged(String),
    AddGameStartArgsChanged(String),
    AddGameDescriptionChanged(String),
    AddGameInstallationInstructionsChanged(String),
    SaveGame,
    CancelAddGame,
    
    // Settings
    OpenSettings,
    SettingsPlayerNameChanged(String),
    SettingsDownloadPathChanged(String),
    SaveSettings,
    CancelSettings,
    
    // License Dialog
    AcceptLicense,
    
    // Periodic updates
    Tick,
    
    // Network Events (from Network thread)
    NetworkEventReceived(DiscoveryEvent),
}

impl DeckDropApp {
    pub fn new() -> Self {
        Self::default()
    }
    
    fn new_with_network_rx(network_event_rx: Arc<std::sync::Mutex<mpsc::Receiver<DiscoveryEvent>>>) -> Self {
        let config = Config::load();
        let mut my_games = Vec::new();
        for game_path in &config.game_paths {
            my_games.extend(deckdrop_core::load_games_from_directory(game_path));
        }
        
        Self {
            current_tab: Tab::MyGames,
            my_games,
            network_games: HashMap::new(),
            peers: Vec::new(),
            active_downloads: HashMap::new(),
            config: config.clone(),
            status: StatusInfo {
                is_online: true,
                peer_count: 0,
                active_download_count: 0,
            },
            _network_event_rx: network_event_rx,
            show_license_dialog: !Config::has_peer_id(),
            show_settings: false,
            show_add_game_dialog: false,
            add_game_path: String::new(),
            add_game_name: String::new(),
            add_game_version: String::new(),
            add_game_start_file: String::new(),
            add_game_start_args: String::new(),
            add_game_description: String::new(),
            add_game_installation_instructions: String::new(),
            settings_player_name: config.player_name.clone(),
            settings_download_path: config.download_path.to_string_lossy().to_string(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabChanged(tab) => {
                self.current_tab = tab;
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
            Message::AddGameInstallationInstructionsChanged(instructions) => {
                self.add_game_installation_instructions = instructions;
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
                    installation_instructions: if self.add_game_installation_instructions.is_empty() { None } else { Some(self.add_game_installation_instructions.clone()) },
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
                for game_path in &self.config.game_paths {
                    self.my_games.extend(deckdrop_core::load_games_from_directory(game_path));
                }
                
                // Close dialog and reset form
                self.show_add_game_dialog = false;
                self.add_game_path = String::new();
                self.add_game_name = String::new();
                self.add_game_version = String::new();
                self.add_game_start_file = String::new();
                self.add_game_start_args = String::new();
                self.add_game_description = String::new();
                self.add_game_installation_instructions = String::new();
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
            Message::SaveSettings => {
                // TODO: Save settings
                self.config.player_name = self.settings_player_name.clone();
                if let Ok(path) = PathBuf::try_from(&self.settings_download_path) {
                    self.config.download_path = path;
                }
                if let Err(e) = self.config.save() {
                    eprintln!("Error saving settings: {}", e);
                }
                self.show_settings = false;
            }
            Message::CancelSettings => {
                self.show_settings = false;
            }
            Message::AcceptLicense => {
                self.show_license_dialog = false;
                self.show_settings = true; // Open Settings after License
            }
            Message::Tick => {
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
    /// Handles network events
    fn handle_network_event(&mut self, event: DiscoveryEvent) {
        match event {
            DiscoveryEvent::PeerFound(peer_info) => {
                if !self.peers.iter().any(|p| p.id == peer_info.id) {
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
            DiscoveryEvent::GameMetadataReceived { peer_id, game_id, deckdrop_toml, deckdrop_chunks_toml } => {
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
        // Check all games in network if they have downloads
        for game_id in self.network_games.keys() {
            if let Ok(manifest_path) = deckdrop_core::get_manifest_path(game_id) {
                if manifest_path.exists() {
                    if let Ok(manifest) = deckdrop_core::DownloadManifest::load(&manifest_path) {
                        // Calculate progress based on progress.percentage
                        let progress_percent = manifest.progress.percentage as f32;
                        
                        // Update or add download state
                        self.active_downloads.insert(game_id.clone(), DownloadState {
                            manifest: manifest.clone(),
                            progress_percent,
                        });
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
                games_column = games_column.push(
                    container(
                        column![
                            text(&game.name).size(18),
                            text(format!("Version: {}", game.version)).size(14),
                            text(format!("Path: {}", game_path.display())).size(12),
                        ]
                        .spacing(5)
                        .padding(15)
                    )
                    .style(container_box_style)
                    .width(Length::Fill)
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
                peers_column = peers_column.push(
                    container(
                        column![
                            text(format!("Peer ID: {}", &peer.id[..16.min(peer.id.len())])).size(14),
                            text(format!("Games: {}", peer.games_count.unwrap_or(0))).size(12),
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
        column![
            text("Settings").size(24),
            text_input("Player Name", &self.config.player_name)
                .on_input(Message::SettingsPlayerNameChanged)
                .padding(10),
            text_input("Download Path", &self.config.download_path.to_string_lossy())
                .on_input(Message::SettingsDownloadPathChanged)
                .padding(10),
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
                text_input("Download Path", &self.settings_download_path)
                    .on_input(Message::SettingsDownloadPathChanged)
                    .padding(10),
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
            column![
                text("Add Game").size(28),
                Space::with_height(20),
                text("Path:").size(16),
                text_input("Path", &self.add_game_path)
                    .on_input(Message::AddGamePathChanged)
                    .padding(10),
                text("Name:").size(16),
                text_input("Name", &self.add_game_name)
                    .on_input(Message::AddGameNameChanged)
                    .padding(10),
                text("Version:").size(16),
                text_input("Version (default: 1.0)", &self.add_game_version)
                    .on_input(Message::AddGameVersionChanged)
                    .padding(10),
                text("Start File:").size(16),
                text_input("Start File (relative to game directory)", &self.add_game_start_file)
                    .on_input(Message::AddGameStartFileChanged)
                    .padding(10),
                text("Start Args (optional):").size(16),
                text_input("Start Args", &self.add_game_start_args)
                    .on_input(Message::AddGameStartArgsChanged)
                    .padding(10),
                text("Description (optional):").size(16),
                text_input("Description", &self.add_game_description)
                    .on_input(Message::AddGameDescriptionChanged)
                    .padding(10),
                text("Installation Instructions (optional):").size(16),
                text_input("Installation Instructions", &self.add_game_installation_instructions)
                    .on_input(Message::AddGameInstallationInstructionsChanged)
                    .padding(10),
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
            .spacing(10)
            .padding(30)
        )
        .width(Length::Fixed(600.0))
        .height(Length::Shrink)
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

