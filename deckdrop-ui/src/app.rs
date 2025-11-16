//! Haupt-App-Struktur für Iced

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, Column, Row, Space},
    Alignment, Element, Length, Theme, Color, Task,
};
use deckdrop_core::{Config, GameInfo, DownloadManifest, DownloadStatus};
use deckdrop_network::network::discovery::DiscoveryEvent;
use deckdrop_network::network::games::NetworkGameInfo;
use deckdrop_network::network::peer::PeerInfo;
use std::collections::HashMap;
use std::path::PathBuf;

/// Haupt-State der Anwendung
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
    
    // Dialoge
    pub show_license_dialog: bool,
    pub show_settings: bool,
    pub show_add_game_dialog: bool,
    
    // Formular-Felder für "Spiel hinzufügen"
    pub add_game_path: String,
    pub add_game_name: String,
    pub add_game_version: String,
    pub add_game_start_file: String,
    pub add_game_start_args: String,
    pub add_game_description: String,
    
    // Settings-Felder
    pub settings_player_name: String,
    pub settings_download_path: String,
}

/// Tab-Auswahl
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    MyGames,
    NetworkGames,
    Peers,
    Settings,
}

/// Download-Status für UI
#[derive(Debug, Clone)]
pub struct DownloadState {
    pub manifest: DownloadManifest,
    pub progress_percent: f32,
}

/// Status-Informationen
#[derive(Debug, Clone)]
pub struct StatusInfo {
    pub is_online: bool,
    pub peer_count: usize,
    pub active_download_count: usize,
}

impl Default for DeckDropApp {
    fn default() -> Self {
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
            show_license_dialog: !Config::has_peer_id(),
            show_settings: false,
            show_add_game_dialog: false,
            add_game_path: String::new(),
            add_game_name: String::new(),
            add_game_version: String::new(),
            add_game_start_file: String::new(),
            add_game_start_args: String::new(),
            add_game_description: String::new(),
            settings_player_name: config.player_name.clone(),
            settings_download_path: config.download_path.to_string_lossy().to_string(),
        }
    }
}

/// Messages für die Anwendung
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
    
    // Meine Spiele
    AddGame,
    AddGamePathChanged(String),
    AddGameNameChanged(String),
    AddGameVersionChanged(String),
    AddGameStartFileChanged(String),
    AddGameStartArgsChanged(String),
    AddGameDescriptionChanged(String),
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
    
    // Periodische Updates
    Tick,
}

impl DeckDropApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabChanged(tab) => {
                self.current_tab = tab;
            }
            Message::NetworkEvent(event) => {
                self.handle_network_event(event);
            }
            Message::DownloadGame(game_id) => {
                // TODO: Download starten
                println!("Download gestartet für: {}", game_id);
            }
            Message::PauseDownload(game_id) => {
                // TODO: Download pausieren
                println!("Download pausiert für: {}", game_id);
            }
            Message::ResumeDownload(game_id) => {
                // TODO: Download fortsetzen
                println!("Download fortgesetzt für: {}", game_id);
            }
            Message::CancelDownload(game_id) => {
                // TODO: Download abbrechen
                println!("Download abgebrochen für: {}", game_id);
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
            Message::SaveGame => {
                // TODO: Spiel speichern
                self.show_add_game_dialog = false;
                // Formular zurücksetzen
                self.add_game_path = String::new();
                self.add_game_name = String::new();
                self.add_game_version = String::new();
                self.add_game_start_file = String::new();
                self.add_game_start_args = String::new();
                self.add_game_description = String::new();
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
                // TODO: Settings speichern
                self.config.player_name = self.settings_player_name.clone();
                if let Ok(path) = PathBuf::try_from(&self.settings_download_path) {
                    self.config.download_path = path;
                }
                if let Err(e) = self.config.save() {
                    eprintln!("Fehler beim Speichern der Einstellungen: {}", e);
                }
                self.show_settings = false;
            }
            Message::CancelSettings => {
                self.show_settings = false;
            }
            Message::AcceptLicense => {
                self.show_license_dialog = false;
                self.show_settings = true; // Öffne Settings nach License
            }
            Message::Tick => {
                // Periodische Updates (z.B. Download-Progress aktualisieren)
                self.update_download_progress();
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<Message> {
        // Haupt-Layout
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
    /// Behandelt Network-Events
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
                // Entferne auch Spiele dieses Peers
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
            DiscoveryEvent::GameMetadataReceived { peer_id, game_id, deckdrop_toml: _, deckdrop_chunks_toml: _ } => {
                // TODO: Download starten
                println!("GameMetadataReceived: {} von {}", game_id, peer_id);
            }
            DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data: _ } => {
                // TODO: Chunk verarbeiten
                println!("ChunkReceived: {} von {}", chunk_hash, peer_id);
            }
            DiscoveryEvent::ChunkRequestFailed { peer_id, chunk_hash, error } => {
                eprintln!("ChunkRequestFailed: {} von {}: {}", chunk_hash, peer_id, error);
            }
        }
    }
    
    /// Aktualisiert Download-Progress
    fn update_download_progress(&mut self) {
        // TODO: Manifeste laden und Progress aktualisieren
        self.status.active_download_count = self.active_downloads.len();
    }
    
    /// Zeigt Tabs
    fn view_tabs(&self) -> Element<Message> {
        row![
            button("Meine Spiele")
                .on_press(Message::TabChanged(Tab::MyGames))
                .style(if self.current_tab == Tab::MyGames {
                    button::primary
                } else {
                    button::secondary
                }),
            button("Spiele im Netzwerk")
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
            button("Einstellungen")
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
    
    /// Zeigt aktuellen Tab
    fn view_current_tab(&self) -> Element<Message> {
        match self.current_tab {
            Tab::MyGames => self.view_my_games(),
            Tab::NetworkGames => self.view_network_games(),
            Tab::Peers => self.view_peers(),
            Tab::Settings => self.view_settings_tab(),
        }
    }
    
    /// Zeigt "Meine Spiele" Tab
    fn view_my_games(&self) -> Element<Message> {
        let mut games_column = Column::new()
            .spacing(10)
            .padding(10);
        
        // Header mit "Spiel hinzufügen" Button
        games_column = games_column.push(
            row![
                text("Meine Spiele").size(24),
                Space::with_width(Length::Fill),
                button("+ Spiel hinzufügen")
                    .on_press(Message::AddGame),
            ]
        );
        
        // Spiele-Liste
        if self.my_games.is_empty() {
            games_column = games_column.push(
                text("Keine Spiele vorhanden. Klicke auf '+ Spiel hinzufügen' um ein Spiel hinzuzufügen.")
            );
        } else {
            for (game_path, game) in &self.my_games {
                games_column = games_column.push(
                    container(
                        column![
                            text(&game.name).size(18),
                            text(format!("Version: {}", game.version)).size(14),
                            text(format!("Pfad: {}", game_path.display())).size(12),
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
    
    /// Zeigt "Spiele im Netzwerk" Tab
    fn view_network_games(&self) -> Element<Message> {
        let mut games_column = Column::new()
            .spacing(10)
            .padding(10);
        
        games_column = games_column.push(
            text("Spiele im Netzwerk").size(24)
        );
        
        if self.network_games.is_empty() {
            games_column = games_column.push(
                text("Keine Spiele im Netzwerk gefunden.")
            );
        } else {
            for (game_id, games) in &self.network_games {
                if let Some((_, game_info)) = games.first() {
                    let is_downloading = self.active_downloads.contains_key(game_id);
                    
                    games_column = games_column.push(
                        container(
                            column![
                                row![
                                    column![
                                        text(&game_info.name).size(18),
                                        text(format!("Version: {}", game_info.version)).size(14),
                                        text(format!("Von: {} Peer(s)", games.len())).size(12),
                                    ]
                                    .width(Length::Fill),
                                    if is_downloading {
                                        button("Download läuft...")
                                            .style(button::secondary)
                                    } else {
                                        button("Get this game")
                                            .on_press(Message::DownloadGame(game_id.clone()))
                                    },
                                ]
                                .spacing(10),
                            ]
                            .spacing(5)
                            .padding(15)
                        )
                        .style(container_box_style)
                        .width(Length::Fill)
                    );
                }
            }
        }
        
        scrollable(games_column)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
    
    /// Zeigt "Peers" Tab
    fn view_peers(&self) -> Element<Message> {
        let mut peers_column = Column::new()
            .spacing(10)
            .padding(10);
        
        peers_column = peers_column.push(
            text("Gefundene Peers").size(24)
        );
        
        if self.peers.is_empty() {
            peers_column = peers_column.push(
                text("Keine Peers gefunden.")
            );
        } else {
            for peer in &self.peers {
                peers_column = peers_column.push(
                    container(
                        column![
                            text(format!("Peer-ID: {}", &peer.id[..16.min(peer.id.len())])).size(14),
                            text(format!("Spiele: {}", peer.games_count.unwrap_or(0))).size(12),
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
    
    /// Zeigt "Einstellungen" Tab
    fn view_settings_tab(&self) -> Element<Message> {
        column![
            text("Einstellungen").size(24),
            text_input("Spielername", &self.config.player_name)
                .on_input(Message::SettingsPlayerNameChanged)
                .padding(10),
            text_input("Download-Pfad", &self.config.download_path.to_string_lossy())
                .on_input(Message::SettingsDownloadPathChanged)
                .padding(10),
            row![
                button("Speichern")
                    .on_press(Message::SaveSettings),
            ]
            .spacing(10),
        ]
        .spacing(15)
        .padding(20)
        .into()
    }
    
    /// Zeigt License-Dialog
    fn view_license_dialog(&self) -> Element<Message> {
        container(
            column![
                text("Willkommen bei DeckDrop").size(28),
                Space::with_height(20),
                text("Bevor du DeckDrop nutzen kannst, musst du den Nutzungsbedingungen zustimmen.").size(16),
                Space::with_height(20),
                scrollable(
                    text("DeckDrop ist eine Peer-to-Peer-Spiele-Sharing-Plattform.\n\n\
                          Durch die Nutzung von DeckDrop erklärst du dich damit einverstanden:\n\n\
                          • Nur Spiele zu teilen, für die du die Rechte besitzt\n\
                          • Keine illegalen Inhalte zu teilen\n\
                          • Die Verantwortung für deine geteilten Inhalte zu übernehmen\n\n\
                          DeckDrop übernimmt keine Haftung für geteilte Inhalte.")
                        .size(14)
                )
                .height(Length::Fixed(200.0)),
                Space::with_height(20),
                button("Akzeptieren")
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
    
    /// Zeigt Settings-Dialog
    fn view_settings(&self) -> Element<Message> {
        container(
            column![
                text("Einstellungen").size(28),
                Space::with_height(20),
                text("Spielername:").size(16),
                text_input("Spielername", &self.settings_player_name)
                    .on_input(Message::SettingsPlayerNameChanged)
                    .padding(10),
                Space::with_height(10),
                text("Download-Pfad:").size(16),
                text_input("Download-Pfad", &self.settings_download_path)
                    .on_input(Message::SettingsDownloadPathChanged)
                    .padding(10),
                Space::with_height(20),
                row![
                    button("Abbrechen")
                        .on_press(Message::CancelSettings),
                    Space::with_width(Length::Fill),
                    button("Speichern")
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
    
    /// Zeigt "Spiel hinzufügen" Dialog
    fn view_add_game_dialog(&self) -> Element<Message> {
        container(
            column![
                text("Spiel hinzufügen").size(28),
                Space::with_height(20),
                text("Pfad:").size(16),
                text_input("Pfad", &self.add_game_path)
                    .on_input(Message::AddGamePathChanged)
                    .padding(10),
                text("Name:").size(16),
                text_input("Name", &self.add_game_name)
                    .on_input(Message::AddGameNameChanged)
                    .padding(10),
                text("Version:").size(16),
                text_input("Version", &self.add_game_version)
                    .on_input(Message::AddGameVersionChanged)
                    .padding(10),
                text("Start-Datei:").size(16),
                text_input("Start-Datei", &self.add_game_start_file)
                    .on_input(Message::AddGameStartFileChanged)
                    .padding(10),
                Space::with_height(20),
                row![
                    button("Abbrechen")
                        .on_press(Message::CancelAddGame),
                    Space::with_width(Length::Fill),
                    button("Speichern")
                        .on_press(Message::SaveGame)
                        .style(button::primary),
                ]
                .width(Length::Fill),
            ]
            .spacing(10)
            .padding(30)
        )
        .width(Length::Fixed(500.0))
        .height(Length::Shrink)
        .style(container_box_style)
        .into()
    }
}

/// Box-Style für Container
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

