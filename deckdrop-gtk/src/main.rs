mod config;
mod settings;
mod game;
mod synch;

use deckdrop_network::network::discovery::{start_discovery, DiscoveryEvent};
use deckdrop_network::network::peer::PeerInfo;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Label, Button, Box as GtkBox, Orientation, Separator, Stack, StackSwitcher, ScrolledWindow, ListBox, ListBoxRow, Entry as GtkEntry, MessageDialog, MessageType, ButtonsType, Window, FileChooserDialog, FileChooserAction, ResponseType, TextView, ProgressBar};
use crate::game::{GameInfo, check_game_config_exists, load_games_from_directory};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::thread;
use std::env;
use crate::config::Config;
use async_channel;

fn main() {
    // Prüfe Command-Line-Argumente VOR GTK-Initialisierung
    let args: Vec<String> = env::args().collect();
    let random_id = args.iter().any(|arg| arg == "--random-id");
    
    // Entferne --random-id aus den Argumenten, damit GTK es nicht als unbekannte Option sieht
    let gtk_args: Vec<String> = args.into_iter()
        .filter(|arg| arg != "--random-id")
        .collect();
    
    // GTK Anwendung
    let app = Application::builder()
        .application_id("com.deckdrop.gtk")
        .build();

    app.connect_activate(move |app| {
        // Wenn --random-id gesetzt ist, generiere eine neue Peer-ID
        if random_id {
            println!("--random-id Flag gesetzt: Generiere neue Peer-ID...");
            
            // Generiere zuerst eine neue Peer-ID (ohne sie zu speichern)
            use libp2p::identity;
            use libp2p::PeerId;
            let keypair = identity::Keypair::generate_ed25519();
            let peer_id = PeerId::from(keypair.public());
            let peer_id_str = peer_id.to_string();
            
            // Setze den Peer-ID-Unterordner BEVOR wir die Konfiguration laden
            Config::set_peer_id_subdir(&peer_id_str);
            
            // Erstelle eine neue Konfiguration mit Standardwerten
            let mut config = Config::default();
            config.peer_id = Some(peer_id_str.clone());
            
            // Speichere die Keypair-Datei im neuen Unterordner
            if let Some(keypair_path) = Config::keypair_path() {
                if let Some(parent) = keypair_path.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        eprintln!("Fehler beim Erstellen des Verzeichnisses: {}", e);
                    }
                }
                match keypair.to_protobuf_encoding() {
                    Ok(bytes) => {
                        if let Err(e) = std::fs::write(&keypair_path, bytes) {
                            eprintln!("Fehler beim Speichern der Keypair: {}", e);
                            app.quit();
                            return;
                        }
                        println!("Neue Peer-ID generiert und gespeichert: {}", peer_id);
                    }
                    Err(e) => {
                        eprintln!("Fehler beim Serialisieren der Keypair: {}", e);
                        app.quit();
                        return;
                    }
                }
            }
            
            // Speichere die neue Konfiguration (mit Standardwerten, leere Spieleliste)
            if let Err(e) = config.save() {
                eprintln!("Fehler beim Speichern der Konfiguration: {}", e);
                app.quit();
                return;
            }
            
            println!("Neue Konfiguration mit Peer-ID {} erstellt", peer_id);
            build_ui(app);
        } else {
            // Prüfe, ob bereits eine Peer-ID existiert
            if !Config::has_peer_id() {
                // Zeige Modal-Dialog für Verantwortungserklärung
                show_license_dialog(app);
            } else {
                build_ui(app);
            }
        }
    });

    // Parse GTK-Argumente manuell, um --random-id zu ignorieren
    let gtk_args_vec: Vec<&str> = gtk_args.iter().map(|s| s.as_str()).collect();
    app.run_with_args(&gtk_args_vec);
}

fn show_license_dialog(app: &Application) {
    // Klone app für den Closure
    let app_clone = app.clone();
    
    // Erstelle ein temporäres Fenster für den Dialog
    let temp_window = ApplicationWindow::builder()
        .application(app)
        .title("DeckDrop")
        .default_width(400)
        .default_height(200)
        .build();
    
    let dialog = MessageDialog::builder()
        .transient_for(&temp_window)
        .modal(true)
        .message_type(MessageType::Warning)
        .text("Verantwortungserklärung")
        .secondary_text("Use only for games you are legally allowed to share. Use on your own responsibility.")
        .buttons(ButtonsType::OkCancel)
        .build();
    
    // Verstecke das temporäre Fenster
    temp_window.hide();
    
    let temp_window_clone = temp_window.clone();
    let app_clone2 = app_clone.clone();
    dialog.connect_response(move |dialog, response| {
        if response == gtk4::ResponseType::Ok {
            // Benutzer hat akzeptiert - generiere Peer-ID
            let mut config = Config::load();
            if let Err(e) = config.generate_and_save_peer_id() {
                eprintln!("Fehler beim Generieren der Peer-ID: {}", e);
                // Zeige Fehler-Dialog
                let error_dialog = MessageDialog::builder()
                    .transient_for(&temp_window_clone)
                    .modal(true)
                    .message_type(MessageType::Error)
                    .text("Fehler")
                    .secondary_text(&format!("Konnte Peer-ID nicht generieren: {}", e))
                    .buttons(ButtonsType::Ok)
                    .build();
                error_dialog.connect_response(|dialog, _| {
                    dialog.close();
                });
                error_dialog.show();
                dialog.close();
                app_clone2.quit();
                return;
            }
            
            dialog.close();
            // Starte die Hauptanwendung
            build_ui(&app_clone2);
        } else {
            // Benutzer hat abgelehnt - schließe Programm
            dialog.close();
            app_clone2.quit();
        }
    });
    
    dialog.show();
}

fn build_ui(app: &Application) {
    // Lade Konfiguration beim Start
    let config = Config::load();
    println!("Geladene Konfiguration: Spielername = {}, Download-Pfad = {}", 
             config.player_name, config.download_path.display());

    // Hauptcontainer
    let main_box = GtkBox::new(Orientation::Vertical, 0);
    
    // Stack für Tabs
    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(200);
    
    // Erstelle Fenster zuerst (wird später gesetzt)
    let window = ApplicationWindow::builder()
        .application(app)
        .title("DeckDrop GTK")
        .default_width(800)
        .default_height(600)
        .build();
    
    // Tab 1: Meine Spiele
    let my_games_box = create_my_games_tab(&config, &window);
    stack.add_titled(&my_games_box, Some("my-games"), "Meine Spiele");
    
    // Tab 2: Spiele im Netzwerk
    let network_games_list = ListBox::new();
    network_games_list.set_selection_mode(gtk4::SelectionMode::None);
    let network_games_scrolled = ScrolledWindow::new();
    network_games_scrolled.set_child(Some(&network_games_list));
    network_games_scrolled.set_hexpand(true);
    network_games_scrolled.set_vexpand(true);
    let network_games_box = GtkBox::new(Orientation::Vertical, 12);
    network_games_box.set_margin_start(20);
    network_games_box.set_margin_end(20);
    network_games_box.set_margin_top(20);
    network_games_box.set_margin_bottom(20);
    network_games_box.append(&network_games_scrolled);
    stack.add_titled(&network_games_box, Some("network-games"), "Spiele im Netzwerk");
    
    // Tab 3: Gefundene Peers
    let peers_list = ListBox::new();
    peers_list.set_selection_mode(gtk4::SelectionMode::None);
    let peers_scrolled = ScrolledWindow::new();
    peers_scrolled.set_child(Some(&peers_list));
    peers_scrolled.set_hexpand(true);
    peers_scrolled.set_vexpand(true);
    stack.add_titled(&peers_scrolled, Some("peers"), "Gefundene Peers");
    
    // Tab 4: Einstellungen
    let settings_box = create_settings_tab(&config);
    stack.add_titled(&settings_box, Some("settings"), "Einstellungen");
    
    // StackSwitcher für Tab-Navigation
    let switcher = StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_halign(gtk4::Align::Center);
    
    // Container für StackSwitcher und Stack
    let content_box = GtkBox::new(Orientation::Vertical, 0);
    content_box.append(&switcher);
    content_box.append(&stack);
    content_box.set_hexpand(true);
    content_box.set_vexpand(true);
    
    main_box.append(&content_box);

    // Separator vor der Statusleiste
    let separator = Separator::new(Orientation::Horizontal);
    main_box.append(&separator);

    // Statusleiste am unteren Rand
    let status_box = GtkBox::new(Orientation::Horizontal, 8);
    status_box.set_margin_start(12);
    status_box.set_margin_end(12);
    status_box.set_margin_top(8);
    status_box.set_margin_bottom(8);
    
    let status_label = Label::new(Some("Status: Online • Peers: 0"));
    status_label.set_halign(gtk4::Align::Start);
    status_label.add_css_class("dim-label");
    status_box.append(&status_label);
    
    main_box.append(&status_box);

    window.set_child(Some(&main_box));
    window.show();

    // Channel zwischen Network (Tokio) und GTK
    let (event_tx, mut event_rx) = mpsc::channel::<DiscoveryEvent>(32);

    // Channel für Download-Requests (vom GTK-Thread zum Tokio-Thread)
    let (download_request_tx, download_request_rx) = tokio::sync::mpsc::unbounded_channel::<deckdrop_network::network::discovery::DownloadRequest>();

    // Async channel für GTK Main Context
    let (glib_tx, glib_rx) = async_channel::unbounded::<DiscoveryEvent>();

    // Tokio Runtime in einem separaten Thread starten, damit sie am Leben bleibt
    let glib_tx_clone = glib_tx.clone();
    let player_name = config.player_name.clone();
    // Berechne die tatsächliche Anzahl der Spiele
    let games_count = {
        let mut count = 0u32;
        for game_path in &config.game_paths {
            if check_game_config_exists(game_path) {
                count += 1;
            }
            if game_path.is_dir() {
                let additional_games = load_games_from_directory(game_path);
                count += additional_games.len() as u32;
            }
        }
        Some(count)
    };
    
    // Lade Keypair für persistente Peer-ID
    let keypair = crate::config::Config::load_keypair();
    
    // Erstelle Callback zum Laden von Spielen
    let games_loader: Option<deckdrop_network::network::discovery::GamesLoader> = {
        use deckdrop_network::network::games::NetworkGameInfo;
        use std::sync::Arc;
        Some(Arc::new(move || {
            let config = Config::load();
            let mut network_games = Vec::new();
            
            // Lade Spiele aus allen in der Config gespeicherten Pfaden
            for game_path in &config.game_paths {
                // Prüfe, ob das Verzeichnis selbst ein Spiel enthält
                if check_game_config_exists(game_path) {
                    if let Ok(game_info) = GameInfo::load_from_path(game_path) {
                        // Konvertiere GameInfo zu NetworkGameInfo
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
                // Lade auch Spiele aus Unterverzeichnissen (falls es ein Verzeichnis ist)
                if game_path.is_dir() {
                    let additional_games = load_games_from_directory(game_path);
                    for (_path, game_info) in additional_games {
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
            }
            
            network_games
        }))
    };
    
    // Erstelle Callback zum Laden von Spiel-Metadaten
    let game_metadata_loader: Option<deckdrop_network::network::discovery::GameMetadataLoader> = {
        use std::sync::Arc;
        Some(Arc::new(move |game_id: &str| {
            let config = Config::load();
            
            // Suche Spiel in allen game_paths
            for game_path in &config.game_paths {
                if check_game_config_exists(game_path) {
                    if let Ok(game_info) = GameInfo::load_from_path(game_path) {
                        if game_info.game_id == game_id {
                            // Lade deckdrop.toml
                            let toml_path = game_path.join("deckdrop.toml");
                            let deckdrop_toml = std::fs::read_to_string(&toml_path).ok()?;
                            
                            // Lade deckdrop_chunks.toml
                            let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                            let deckdrop_chunks_toml = std::fs::read_to_string(&chunks_toml_path).ok()?;
                            
                            return Some((deckdrop_toml, deckdrop_chunks_toml));
                        }
                    }
                }
                
                // Prüfe auch Unterverzeichnisse
                if game_path.is_dir() {
                    let additional_games = load_games_from_directory(game_path);
                    for (path, game_info) in additional_games {
                        if game_info.game_id == game_id {
                            let toml_path = path.join("deckdrop.toml");
                            let deckdrop_toml = std::fs::read_to_string(&toml_path).ok()?;
                            
                            let chunks_toml_path = path.join("deckdrop_chunks.toml");
                            let deckdrop_chunks_toml = std::fs::read_to_string(&chunks_toml_path).ok()?;
                            
                            return Some((deckdrop_toml, deckdrop_chunks_toml));
                        }
                    }
                }
            }
            
            None
        }))
    };
    
    // Erstelle Callback zum Laden von Chunks
    let chunk_loader: Option<deckdrop_network::network::discovery::ChunkLoader> = {
        use std::sync::Arc;
        Some(Arc::new(move |chunk_hash: &str| {
            let config = Config::load();
            
            // Neues Format: "{file_hash}:{chunk_index}"
            let parts: Vec<&str> = chunk_hash.split(':').collect();
            if parts.len() != 2 {
                eprintln!("Ungültiges Chunk-Hash-Format: {}", chunk_hash);
                return None;
            }
            
            let file_hash = parts[0];
            let chunk_index = parts[1].parse::<usize>().ok()?;
            
            // Suche Chunk in allen game_paths
            for game_path in &config.game_paths {
                // Prüfe ob Spiel existiert
                if check_game_config_exists(game_path) {
                    // Lade deckdrop_chunks.toml um Datei mit diesem file_hash zu finden
                    let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
                    if let Ok(chunks_toml_content) = std::fs::read_to_string(&chunks_toml_path) {
                        // Parse chunks.toml
                        if let Ok(chunks_data) = toml::from_str::<Vec<serde_json::Value>>(&chunks_toml_content) {
                            for entry in chunks_data {
                                // Prüfe ob file_hash übereinstimmt
                                if let Some(entry_file_hash) = entry.get("file_hash").and_then(|h| h.as_str()) {
                                    if entry_file_hash == file_hash {
                                        // Datei gefunden - extrahiere Chunk basierend auf Index
                                        if let Some(file_path) = entry.get("path").and_then(|p| p.as_str()) {
                                            let full_path = game_path.join(file_path);
                                            if let Ok(mut file) = std::fs::File::open(&full_path) {
                                                use std::io::{Read, Seek, SeekFrom};
                                                const CHUNK_SIZE: usize = 100 * 1024 * 1024; // 100MB
                                                
                                                // Springe zum richtigen Chunk
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
                }
            }
            
            None
        }))
    };
    
    thread::spawn(move || {
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        
        // Network Discovery Task
        rt.spawn(async move {
            let _handle = start_discovery(
                event_tx.clone(), 
                Some(player_name.clone()), 
                games_count, 
                keypair, 
                games_loader,
                game_metadata_loader,
                chunk_loader,
                Some(download_request_rx),
            ).await;
            println!("Discovery gestartet, warte auf Events...");
            eprintln!("Discovery gestartet, warte auf Events...");

            while let Some(event) = event_rx.recv().await {
                println!("Event empfangen im Tokio Thread: {:?}", event);
                eprintln!("Event empfangen im Tokio Thread: {:?}", event);
                
                // Prüfe, ob es ein GamesListReceived Event ist
                if let DiscoveryEvent::GamesListReceived { peer_id, games } = &event {
                    println!("*** GamesListReceived Event im Tokio Thread: {} Spiele von {} ***", games.len(), peer_id);
                    eprintln!("*** GamesListReceived Event im Tokio Thread: {} Spiele von {} ***", games.len(), peer_id);
                }
                
                // Events in den GTK Thread schicken
                match glib_tx_clone.send(event).await {
                    Ok(()) => {
                        println!("Event erfolgreich an GTK Thread gesendet");
                        eprintln!("Event erfolgreich an GTK Thread gesendet");
                    }
                    Err(e) => {
                        eprintln!("FEHLER beim Senden des Events an GTK Thread: {}", e);
                        println!("FEHLER beim Senden des Events an GTK Thread: {}", e);
                    }
                }
            }
        });
        
        // Runtime am Leben halten
        rt.block_on(async {
            // Laufende Tasks am Leben halten
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });
    });

    // Events im GTK Thread verarbeiten
    let main_context = glib::MainContext::default();
    let known_peers: Rc<RefCell<HashMap<String, PeerInfo>>> = Rc::new(RefCell::new(HashMap::new()));
    let known_peers_clone = known_peers.clone();
    
    // Clone weak references for the async closure
    let peers_list_weak = peers_list.downgrade();
    let status_label_weak = status_label.downgrade();
    let network_games_list_weak = network_games_list.downgrade();
    
    // Speichere Netzwerk-Spiele nach game_id gruppiert
    use deckdrop_network::network::games::NetworkGameInfo;
    let network_games: Rc<RefCell<HashMap<String, Vec<(String, NetworkGameInfo)>>>> = Rc::new(RefCell::new(HashMap::new()));
    let network_games_clone = network_games.clone();
    
    // Aktive Downloads tracken
    let active_downloads: ActiveDownloads = Rc::new(RefCell::new(HashMap::new()));
    let active_downloads_clone = active_downloads.clone();
    
    // Clone download_request_tx für Button-Handler
    let download_request_tx_clone = download_request_tx.clone();
    
    let download_request_tx_for_async = download_request_tx.clone();
    main_context.spawn_local_with_priority(glib::Priority::default(), async move {
        println!("GTK Event-Handler gestartet, warte auf Events...");
        eprintln!("GTK Event-Handler gestartet, warte auf Events...");
        
        let download_request_tx = download_request_tx_for_async;
        
        while let Ok(event) = glib_rx.recv().await {
            println!("Event empfangen im GTK Thread: {:?}", event);
            eprintln!("Event empfangen im GTK Thread: {:?}", event);
            
            // Upgrade weak references
            let Some(peers_list) = peers_list_weak.upgrade() else { 
                eprintln!("peers_list weak reference ist nicht mehr verfügbar");
                break;
            };
            let Some(status_label) = status_label_weak.upgrade() else { 
                eprintln!("status_label weak reference ist nicht mehr verfügbar");
                break;
            };
            
            // Prüfe, ob network_games_list verfügbar ist, bevor wir Events verarbeiten
            let network_games_list_available = network_games_list_weak.upgrade().is_some();
            if !network_games_list_available {
                eprintln!("WARNUNG: network_games_list nicht verfügbar beim Event-Handling!");
            }
            
            match event {
                DiscoveryEvent::PeerFound(peer) => {
                    // Prüfe ob Peer bereits existiert und ob sich die Metadaten geändert haben
                    let mut needs_update = false;
                    let was_new = {
                        let mut peers = known_peers_clone.borrow_mut();
                        let existing = peers.get(&peer.id);
                        let was_new = existing.is_none();
                        
                        // Prüfe ob sich Metadaten geändert haben
                        if let Some(existing_peer) = existing {
                            needs_update = existing_peer.player_name != peer.player_name 
                                || existing_peer.games_count != peer.games_count;
                        }
                        
                        peers.insert(peer.id.clone(), peer.clone());
                        was_new
                    };
                    
                    if was_new {
                        println!("Neuer Peer gefunden: {} (Gesamt: {})", peer.id, known_peers_clone.borrow().len());
                        update_peers_list(&peers_list, &known_peers_clone);
                    } else if needs_update {
                        println!("Peer-Metadaten aktualisiert: {} (name: {:?}, games: {:?})", 
                            peer.id, peer.player_name, peer.games_count);
                        update_peers_list(&peers_list, &known_peers_clone);
                    }
                    let peer_count = known_peers_clone.borrow().len();
                    status_label.set_text(&format!("Status: Online • Peers: {}", peer_count));
                }
                DiscoveryEvent::PeerLost(peer_id) => {
                    let was_removed = known_peers_clone.borrow_mut().remove(&peer_id).is_some();
                    if was_removed {
                        println!("Peer verloren: {} (Gesamt: {})", peer_id, known_peers_clone.borrow().len());
                        update_peers_list(&peers_list, &known_peers_clone);
                        
                        // Entferne auch die Spiele dieses Peers
                        {
                            let mut games = network_games_clone.borrow_mut();
                            games.retain(|_, peer_games| {
                                peer_games.retain(|(p_id, _)| p_id != &peer_id);
                                !peer_games.is_empty()
                            });
                        } // mutable borrow endet hier
                        
                        if let Some(network_games_list) = network_games_list_weak.upgrade() {
                            update_network_games_list(&network_games_list, &network_games_clone, &download_request_tx, &active_downloads_clone);
                        }
                    }
                    let peer_count = known_peers_clone.borrow().len();
                    status_label.set_text(&format!("Status: Online • Peers: {}", peer_count));
                }
                DiscoveryEvent::GameMetadataReceived { peer_id, game_id, deckdrop_toml, deckdrop_chunks_toml } => {
                    println!("=== GameMetadataReceived Event im GTK Thread ===");
                    eprintln!("=== GameMetadataReceived Event im GTK Thread ===");
                    println!("Metadaten erhalten für Spiel {} von Peer {}", game_id, peer_id);
                    eprintln!("Metadaten erhalten für Spiel {} von Peer {}", game_id, peer_id);
                    
                    // Erstelle Download-UI falls noch nicht vorhanden
                    if !active_downloads_clone.borrow().contains_key(&game_id) {
                        create_download_ui(&game_id, &active_downloads_clone, &download_request_tx_clone, &network_games_list_weak, &network_games_clone);
                    }
                    
                    // Starte Download-Prozess
                    if let Err(e) = crate::synch::start_game_download(&game_id, &deckdrop_toml, &deckdrop_chunks_toml) {
                        eprintln!("Fehler beim Starten des Downloads: {}", e);
                    } else {
                        // Nach erfolgreichem Manifest-Erstellen: Starte Chunk-Downloads
                        if let Err(e) = crate::synch::request_missing_chunks(
                            &game_id,
                            &[peer_id.clone()],
                            &download_request_tx_clone,
                        ) {
                            eprintln!("Fehler beim Anfordern fehlender Chunks: {}", e);
                        }
                    }
                }
                DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data } => {
                    println!("=== ChunkReceived Event im GTK Thread ===");
                    eprintln!("=== ChunkReceived Event im GTK Thread ===");
                    println!("Chunk {} erhalten von Peer {}: {} Bytes", chunk_hash, peer_id, chunk_data.len());
                    eprintln!("Chunk {} erhalten von Peer {}: {} Bytes", chunk_hash, peer_id, chunk_data.len());
                    
                    // Finde game_id durch Suche in allen Manifesten
                    if let Ok(game_id) = crate::synch::find_game_id_for_chunk(&chunk_hash) {
                        // Prüfe ob Download pausiert ist
                        let manifest_path = match crate::synch::get_manifest_path(&game_id) {
                            Ok(path) => path,
                            Err(e) => {
                                eprintln!("Fehler beim Ermitteln des Manifest-Pfads: {}", e);
                                continue;
                            }
                        };
                        
                        let manifest = match crate::synch::DownloadManifest::load(&manifest_path) {
                            Ok(m) => m,
                            Err(e) => {
                                eprintln!("Fehler beim Laden des Manifests: {}", e);
                                continue;
                            }
                        };
                        
                        // Überspringe wenn pausiert
                        if manifest.overall_status == crate::synch::DownloadStatus::Paused {
                            println!("Download pausiert, überspringe Chunk {}", chunk_hash);
                            continue;
                        }
                        
                        // Speichere Chunk
                        let chunks_dir = match crate::synch::get_chunks_dir(&game_id) {
                            Ok(dir) => dir,
                            Err(e) => {
                                eprintln!("Fehler beim Ermitteln des Chunks-Verzeichnisses: {}", e);
                                continue;
                            }
                        };
                        
                        if let Err(e) = crate::synch::save_chunk(&chunk_hash, &chunk_data, &chunks_dir) {
                            eprintln!("Fehler beim Speichern des Chunks {}: {}", chunk_hash, e);
                            continue;
                        }
                        
                        // Aktualisiere Manifest
                        let mut manifest = match crate::synch::DownloadManifest::load(&manifest_path) {
                            Ok(m) => m,
                            Err(e) => {
                                eprintln!("Fehler beim Laden des Manifests: {}", e);
                                continue;
                            }
                        };
                        
                        manifest.mark_chunk_downloaded(&chunk_hash);
                        
                        if let Err(e) = manifest.save(&manifest_path) {
                            eprintln!("Fehler beim Speichern des Manifests: {}", e);
                            continue;
                        }
                        
                        // Aktualisiere UI
                        update_download_ui(&game_id, &active_downloads_clone);
                        
                        // Prüfe ob Dateien komplett sind und rekonstruiere sie
                        if let Err(e) = crate::synch::check_and_reconstruct_files(&game_id, &manifest) {
                            eprintln!("Fehler beim Rekonstruieren von Dateien: {}", e);
                        }
                        
                        // Prüfe ob Spiel komplett ist
                        if manifest.overall_status == crate::synch::DownloadStatus::Complete {
                            if let Err(e) = crate::synch::finalize_game_download(&game_id, &manifest) {
                                eprintln!("Fehler bei der Spiel-Finalisierung: {}", e);
                            }
                            // Entferne aus aktiven Downloads
                            active_downloads_clone.borrow_mut().remove(&game_id);
                            // Aktualisiere Liste
                            if let Some(network_games_list) = network_games_list_weak.upgrade() {
                                update_network_games_list(&network_games_list, &network_games_clone, &download_request_tx_clone, &active_downloads_clone);
                            }
                        }
                    } else {
                        eprintln!("Konnte game_id für Chunk {} nicht finden", chunk_hash);
                    }
                }
                DiscoveryEvent::ChunkRequestFailed { peer_id, chunk_hash, error } => {
                    eprintln!("Chunk-Request fehlgeschlagen: {} von {}: {}", chunk_hash, peer_id, error);
                    // TODO: Versuche anderen Peer
                }
                DiscoveryEvent::GamesListReceived { peer_id, games } => {
                    println!("=== GamesListReceived Event im GTK Thread ===");
                    eprintln!("=== GamesListReceived Event im GTK Thread ===");
                    println!("Spiele-Liste erhalten von {}: {} Spiele", peer_id, games.len());
                    eprintln!("Spiele-Liste erhalten von {}: {} Spiele", peer_id, games.len());
                    
                    // Aktualisiere die Netzwerk-Spiele-Liste
                    // Gruppiere nach game_id + version (unique_key)
                    let games_count = {
                        let mut network_games_map = network_games_clone.borrow_mut();
                        for game in games {
                            let key = game.unique_key();
                            println!("Füge Spiel hinzu: {} (key: {})", game.name, key);
                            eprintln!("Füge Spiel hinzu: {} (key: {})", game.name, key);
                            network_games_map
                                .entry(key)
                                .or_insert_with(Vec::new)
                                .push((peer_id.clone(), game));
                        }
                        
                        let count = network_games_map.len();
                        println!("Aktuelle Anzahl Spiele in network_games_map: {}", count);
                        eprintln!("Aktuelle Anzahl Spiele in network_games_map: {}", count);
                        count
                    }; // mutable borrow endet hier
                    
                    // Versuche die Liste zu aktualisieren
                    match network_games_list_weak.upgrade() {
                        Some(network_games_list) => {
                            println!("Aktualisiere Netzwerk-Spiele-Liste...");
                            eprintln!("Aktualisiere Netzwerk-Spiele-Liste...");
                            update_network_games_list(&network_games_list, &network_games_clone, &download_request_tx, &active_downloads_clone);
                            println!("Netzwerk-Spiele-Liste aktualisiert!");
                            eprintln!("Netzwerk-Spiele-Liste aktualisiert!");
                        }
                        None => {
                            eprintln!("FEHLER: network_games_list weak reference ist nicht mehr verfügbar!");
                            println!("FEHLER: network_games_list weak reference ist nicht mehr verfügbar!");
                        }
                    }
                }
            }
        }
    });
}

fn create_my_games_tab(config: &Config, window: &ApplicationWindow) -> GtkBox {
    let box_ = GtkBox::new(Orientation::Vertical, 12);
    box_.set_margin_start(20);
    box_.set_margin_end(20);
    box_.set_margin_top(20);
    box_.set_margin_bottom(20);
    
    // Header mit Download-Pfad und Button
    let header_box = GtkBox::new(Orientation::Horizontal, 12);
    header_box.set_halign(gtk4::Align::Fill);
    
    let label = Label::new(Some(&format!("Download-Pfad: {}", config.download_path.display())));
    label.set_halign(gtk4::Align::Start);
    label.set_hexpand(true);
    header_box.append(&label);
    
    let add_game_button = Button::with_label("Spiel hinzufügen");
    add_game_button.add_css_class("suggested-action");
    add_game_button.set_halign(gtk4::Align::End);
    header_box.append(&add_game_button);
    
    box_.append(&header_box);
    
    // Liste der Spiele
    let games_list = ListBox::new();
    games_list.set_selection_mode(gtk4::SelectionMode::None);
    let games_scrolled = ScrolledWindow::new();
    games_scrolled.set_child(Some(&games_list));
    games_scrolled.set_hexpand(true);
    games_scrolled.set_vexpand(true);
    box_.append(&games_scrolled);
    
    // Lade vorhandene Spiele
    update_games_list(&games_list, &config.download_path);
    
    // Event-Handler für Button
    let download_path = config.download_path.clone();
    let games_list_clone = games_list.clone();
    let window_clone = window.clone();
    add_game_button.connect_clicked(move |_| {
        show_add_game_dialog(&window_clone, &download_path, &games_list_clone);
    });
    
    box_
}

fn show_add_game_dialog(parent: &ApplicationWindow, download_path: &std::path::PathBuf, games_list: &ListBox) {
    let dialog = Window::builder()
        .title("Spiel hinzufügen")
        .default_width(500)
        .default_height(500)
        .modal(true)
        .transient_for(parent)
        .build();
    
    let content_box = GtkBox::new(Orientation::Vertical, 12);
    content_box.set_margin_start(20);
    content_box.set_margin_end(20);
    content_box.set_margin_top(20);
    content_box.set_margin_bottom(20);
    
    // Spielpfad
    let path_label = Label::new(Some("Spielpfad:"));
    path_label.set_halign(gtk4::Align::Start);
    content_box.append(&path_label);
    
    let path_box = GtkBox::new(Orientation::Horizontal, 8);
    let path_entry = GtkEntry::new();
    path_entry.set_hexpand(true);
    path_box.append(&path_entry);
    
    let browse_button = Button::with_label("Durchsuchen...");
    browse_button.set_halign(gtk4::Align::End);
    path_box.append(&browse_button);
    content_box.append(&path_box);
    
    // Name
    let name_label = Label::new(Some("Name:"));
    name_label.set_halign(gtk4::Align::Start);
    name_label.set_margin_top(12);
    content_box.append(&name_label);
    
    let name_entry = GtkEntry::new();
    name_entry.set_hexpand(true);
    content_box.append(&name_entry);
    
    // Version
    let version_label = Label::new(Some("Version:"));
    version_label.set_halign(gtk4::Align::Start);
    version_label.set_margin_top(12);
    content_box.append(&version_label);
    
    let version_entry = GtkEntry::new();
    version_entry.set_text("1.0");
    version_entry.set_hexpand(true);
    content_box.append(&version_entry);
    
    // Spielstart-Datei
    let start_file_label = Label::new(Some("Spielstart-Datei (relativ zum Spielverzeichnis):"));
    start_file_label.set_halign(gtk4::Align::Start);
    start_file_label.set_margin_top(12);
    content_box.append(&start_file_label);
    
    let start_file_entry = GtkEntry::new();
    start_file_entry.set_hexpand(true);
    content_box.append(&start_file_entry);
    
    // Spielstart-Argumente
    let start_args_label = Label::new(Some("Spielstart-Argumente (optional):"));
    start_args_label.set_halign(gtk4::Align::Start);
    start_args_label.set_margin_top(12);
    content_box.append(&start_args_label);
    
    let start_args_entry = GtkEntry::new();
    start_args_entry.set_hexpand(true);
    content_box.append(&start_args_entry);
    
    // Beschreibung
    let description_label = Label::new(Some("Beschreibung (optional):"));
    description_label.set_halign(gtk4::Align::Start);
    description_label.set_margin_top(12);
    content_box.append(&description_label);
    
    let description_text_view = TextView::new();
    description_text_view.set_hexpand(true);
    description_text_view.set_vexpand(false);
    let description_scrolled = ScrolledWindow::new();
    description_scrolled.set_child(Some(&description_text_view));
    description_scrolled.set_min_content_height(80);
    description_scrolled.set_hexpand(true);
    content_box.append(&description_scrolled);
    
    // Fortschrittsanzeige (zunächst versteckt)
    let progress_label = Label::new(Some(""));
    progress_label.set_halign(gtk4::Align::Start);
    progress_label.set_margin_top(12);
    progress_label.set_visible(false);
    content_box.append(&progress_label);
    
    let progress_bar = ProgressBar::new();
    progress_bar.set_show_text(true);
    progress_bar.set_hexpand(true);
    progress_bar.set_visible(false);
    progress_bar.set_margin_top(6);
    content_box.append(&progress_bar);
    
    // Button-Box
    let button_box = GtkBox::new(Orientation::Horizontal, 8);
    button_box.set_halign(gtk4::Align::End);
    button_box.set_margin_top(20);
    
    let cancel_button = Button::with_label("Abbrechen");
    let save_button = Button::with_label("Speichern");
    save_button.add_css_class("suggested-action");
    
    button_box.append(&cancel_button);
    button_box.append(&save_button);
    content_box.append(&button_box);
    
    dialog.set_child(Some(&content_box));
    
    // Event-Handler für Browse-Button
    let download_path_clone = download_path.clone();
    let games_list_clone = games_list.clone();
    let dialog_clone = dialog.clone();
    let path_entry_clone = path_entry.clone();
    browse_button.connect_clicked(move |_| {
        let file_dialog = FileChooserDialog::builder()
            .title("Spielverzeichnis auswählen")
            .action(FileChooserAction::SelectFolder)
            .transient_for(&dialog_clone)
            .build();
        
        // Füge OK und Abbrechen Buttons hinzu
        file_dialog.add_button("Abbrechen", ResponseType::Cancel);
        file_dialog.add_button("Auswählen", ResponseType::Accept);
        
        let download_path_clone2 = download_path_clone.clone();
        let games_list_clone2 = games_list_clone.clone();
        let dialog_clone2 = dialog_clone.clone();
        let path_entry_clone2 = path_entry_clone.clone();
        file_dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        // Prüfe ob bereits eine valide TOML existiert
                        if check_game_config_exists(&path) {
                            if GameInfo::load_from_path(&path).is_ok() {
                                // Valide TOML gefunden - Spiel direkt hinzufügen ohne Dialog
                                // Die Peer-ID bleibt die vom Ersteller (bereits in der TOML)
                                // Füge den Pfad zur Config hinzu
                                let mut config = Config::load();
                                if let Err(e) = config.add_game_path(&path) {
                                    eprintln!("Fehler beim Hinzufügen des Spiel-Pfads zur Config: {}", e);
                                }
                                // Das Spiel wird automatisch zur Liste hinzugefügt
                                dialog_clone2.close();
                                update_games_list(&games_list_clone2, &download_path_clone2);
                                dialog.close();
                                return;
                            }
                        }
                        // Keine valide TOML - Pfad in das Eingabefeld eintragen
                        path_entry_clone2.set_text(&path.to_string_lossy());
                    }
                }
            }
            dialog.close();
        });
        
        file_dialog.show();
    });
    
    // Event-Handler für Cancel-Button
    let dialog_clone = dialog.clone();
    cancel_button.connect_clicked(move |_| {
        dialog_clone.close();
    });
    
    // Event-Handler für Save-Button
    let download_path_clone = download_path.clone();
    let games_list_clone = games_list.clone();
    let dialog_clone = dialog.clone();
    let description_text_view_clone = description_text_view.clone();
    let progress_label_clone = progress_label.clone();
    let progress_bar_clone = progress_bar.clone();
    let save_button_clone = save_button.clone();
    let config = Config::load();
    let peer_id = config.peer_id.clone();
    save_button.connect_clicked(move |_| {
        let game_path_str = path_entry.text().to_string();
        let name = name_entry.text().to_string();
        let version = version_entry.text().to_string();
        let start_file = start_file_entry.text().to_string();
        let start_args = start_args_entry.text().to_string();
        
        // Hole Description aus TextView
        let description_buffer = description_text_view_clone.buffer();
        let (start_iter, end_iter) = description_buffer.bounds();
        let description = description_buffer.text(&start_iter, &end_iter, false).to_string();
        
        // Validierung
        if game_path_str.is_empty() {
            show_error_dialog(&dialog_clone, "Bitte gib einen Spielpfad an.");
            return;
        }
        
        if name.is_empty() {
            show_error_dialog(&dialog_clone, "Bitte gib einen Namen an.");
            return;
        }
        
        if start_file.is_empty() {
            show_error_dialog(&dialog_clone, "Bitte gib eine Spielstart-Datei an.");
            return;
        }
        
        let game_path = std::path::PathBuf::from(&game_path_str);
        
        // Zeige Fortschrittsanzeige
        progress_label_clone.set_visible(true);
        progress_bar_clone.set_visible(true);
        progress_label_clone.set_text("Initialisiere Chunk-Generierung...");
        progress_bar_clone.set_fraction(0.0);
        progress_bar_clone.set_text(Some("0%"));
        
        // Deaktiviere Save-Button während der Verarbeitung
        save_button_clone.set_sensitive(false);
        
        // Erstelle GameInfo mit Peer-ID (wird später gesetzt)
        let game_id = crate::game::generate_game_id();
        let game_info = GameInfo {
            game_id: game_id.clone(),
            name: name.clone(),
            version: if version.is_empty() { "1.0".to_string() } else { version },
            start_file,
            start_args: if start_args.is_empty() { None } else { Some(start_args) },
            description: if description.trim().is_empty() { None } else { Some(description.trim().to_string()) },
            creator_peer_id: peer_id.clone(),
            hash: None, // Wird später gesetzt, nachdem deckdrop_chunks.toml generiert wurde
        };
        
        // Channel für Progress-Updates (String = "progress:current:total:filename" oder "done:hash" oder "error:message")
        let (progress_tx, progress_rx) = async_channel::unbounded::<String>();
        let progress_label_for_thread = progress_label_clone.clone();
        let progress_bar_for_thread = progress_bar_clone.clone();
        let save_button_for_thread = save_button_clone.clone();
        let dialog_clone_for_thread = dialog_clone.clone();
        let games_list_clone_for_thread = games_list_clone.clone();
        let download_path_clone_for_thread = download_path_clone.clone();
        let game_path_for_ui = game_path.clone();
        let game_info_for_ui = game_info.clone();
        let game_path_for_thread = game_path.clone();
        let game_info_for_thread = game_info.clone();
        
        // Spawn Progress-Update-Handler im GTK Main Context
        // Wir sind bereits im Main Context, daher können wir Widgets direkt aktualisieren
        let main_context = glib::MainContext::default();
        main_context.spawn_local(async move {
            while let Ok(message) = progress_rx.recv().await {
                println!("Progress-Message empfangen: {}", message);
                eprintln!("Progress-Message empfangen: {}", message);
                
                if message.starts_with("progress:") {
                    // Format: "progress:current:total:filename"
                    let parts: Vec<&str> = message.splitn(4, ':').collect();
                    if parts.len() == 4 {
                        if let (Ok(current), Ok(total)) = (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                            let file_name = parts[3].to_string();
                            let percentage = if total > 0 {
                                (current as f64 / total as f64) * 100.0
                            } else {
                                0.0
                            };
                            println!("Update Progress: {}/{} = {:.1}%", current, total, percentage);
                            eprintln!("Update Progress: {}/{} = {:.1}%", current, total, percentage);
                            
                            // Wir sind bereits im Main Context - direkte UI-Updates
                            progress_bar_for_thread.set_fraction(percentage / 100.0);
                            progress_bar_for_thread.set_text(Some(&format!("{:.1}%", percentage)));
                            progress_label_for_thread.set_text(&format!("Verarbeite Datei {}/{}: {}", current, total, file_name));
                        }
                    }
                } else if message.starts_with("done:") {
                    let hash = message.strip_prefix("done:").unwrap_or("");
                    println!("Chunk-Generierung abgeschlossen, Hash: {}", hash);
                    eprintln!("Chunk-Generierung abgeschlossen, Hash: {}", hash);
                    
                    // Wir sind bereits im Main Context - direkte UI-Updates
                    progress_bar_for_thread.set_fraction(1.0);
                    progress_bar_for_thread.set_text(Some("100%"));
                    progress_label_for_thread.set_text("Chunk-Generierung abgeschlossen. Speichere Spiel...");
                    
                    // Speichere TOML mit dem Hash der deckdrop_chunks.toml
                    let chunks_hash = if hash.is_empty() { None } else { Some(hash.to_string()) };
                    match game_info_for_ui.save_to_path_with_hash(&game_path_for_ui, chunks_hash) {
                        Ok(_) => {
                            // Füge den Spiel-Pfad zur Config hinzu
                            let mut config = Config::load();
                            if let Err(e) = config.add_game_path(&game_path_for_ui) {
                                eprintln!("Fehler beim Hinzufügen des Spiel-Pfads zur Config: {}", e);
                            }
                            
                            dialog_clone_for_thread.close();
                            // Aktualisiere die Liste mit den Pfaden aus der Config
                            update_games_list(&games_list_clone_for_thread, &download_path_clone_for_thread);
                        }
                        Err(e) => {
                            progress_label_for_thread.set_text(&format!("Fehler beim Speichern: {}", e));
                            save_button_for_thread.set_sensitive(true);
                            show_error_dialog(&dialog_clone_for_thread, &format!("Fehler beim Speichern: {}", e));
                        }
                    }
                } else if message.starts_with("error:") {
                    let error_msg = message.strip_prefix("error:").unwrap_or("Unbekannter Fehler");
                    println!("Fehler: {}", error_msg);
                    eprintln!("Fehler: {}", error_msg);
                    
                    // Wir sind bereits im Main Context - direkte UI-Updates
                    progress_label_for_thread.set_text(&format!("Fehler: {}", error_msg));
                    save_button_for_thread.set_sensitive(true);
                }
            }
        });
        
        // Führe Chunk-Generierung in separatem Thread aus
        thread::spawn(move || {
            let game_path_for_chunks = game_path_for_thread.clone();
            println!("Starte Chunk-Generierung für: {}", game_path_for_chunks.display());
            eprintln!("Starte Chunk-Generierung für: {}", game_path_for_chunks.display());
            
            // Generiere deckdrop_chunks.toml mit Progress-Callback
            let progress_tx_clone = progress_tx.clone();
            let chunks_hash = match crate::game::generate_chunks_toml(&game_path_for_chunks, Some(move |current: usize, total: usize, file_name: &str| {
                let _ = progress_tx_clone.send(format!("progress:{}:{}:{}", current, total, file_name));
            })) {
                Ok(hash) => {
                    println!("Chunk-Generierung abgeschlossen. Hash: {}", hash);
                    eprintln!("Chunk-Generierung abgeschlossen. Hash: {}", hash);
                    Some(hash)
                }
                Err(e) => {
                    eprintln!("Fehler beim Generieren von deckdrop_chunks.toml: {}", e);
                    let _ = progress_tx.send(format!("error:{}", e));
                    None
                }
            };
            
            // Sende Completion-Signal
            if let Some(hash) = chunks_hash {
                let _ = progress_tx.send(format!("done:{}", hash));
            } else {
                let _ = progress_tx.send("done:".to_string());
            }
        });
    });
    
    dialog.show();
}

fn show_error_dialog(parent: &Window, message: &str) {
    let error_dialog = MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(MessageType::Error)
        .text("Fehler")
        .secondary_text(message)
        .buttons(ButtonsType::Ok)
        .build();
    
    error_dialog.connect_response(|dialog, _| {
        dialog.close();
    });
    
    error_dialog.show();
}

fn update_games_list(games_list: &ListBox, _download_path: &std::path::PathBuf) {
    // Entferne alle vorhandenen Zeilen
    while let Some(row) = games_list.row_at_index(0) {
        games_list.remove(&row);
    }
    
    // Lade Config mit Spiel-Pfaden
    let config = Config::load();
    let mut games = Vec::new();
    
    // Lade Spiele aus allen in der Config gespeicherten Pfaden
    for game_path in &config.game_paths {
        // Prüfe, ob das Verzeichnis selbst ein Spiel enthält
        if check_game_config_exists(game_path) {
            if let Ok(game_info) = GameInfo::load_from_path(game_path) {
                games.push((game_path.clone(), game_info));
            }
        }
        // Lade auch Spiele aus Unterverzeichnissen (falls es ein Verzeichnis ist)
        if game_path.is_dir() {
            let additional_games = load_games_from_directory(game_path);
            games.extend(additional_games);
        }
    }
    
    // Entferne Duplikate (basierend auf Pfad)
    games.sort_by(|a, b| a.0.cmp(&b.0));
    games.dedup_by(|a, b| a.0 == b.0);
    
    if games.is_empty() {
        let row = ListBoxRow::new();
        let label = Label::new(Some("Keine Spiele gefunden. Klicke auf 'Spiel hinzufügen' um ein Spiel hinzuzufügen."));
        label.set_halign(gtk4::Align::Center);
        label.add_css_class("dim-label");
        row.set_child(Some(&label));
        games_list.append(&row);
    } else {
        for (path, game_info) in games {
            let row = ListBoxRow::new();
            let row_box = GtkBox::new(Orientation::Vertical, 6);
            row_box.set_margin_start(12);
            row_box.set_margin_end(12);
            row_box.set_margin_top(12);
            row_box.set_margin_bottom(12);
            
            // Name
            let name_label = Label::new(Some(&game_info.name));
            name_label.set_halign(gtk4::Align::Start);
            name_label.set_xalign(0.0);
            name_label.add_css_class("title-4");
            row_box.append(&name_label);
            
            // Version
            let version_label = Label::new(Some(&format!("Version: {}", game_info.version)));
            version_label.set_halign(gtk4::Align::Start);
            version_label.set_xalign(0.0);
            row_box.append(&version_label);
            
            // Beschreibung (falls vorhanden)
            if let Some(ref description) = game_info.description {
                let description_label = Label::new(Some(description));
                description_label.set_halign(gtk4::Align::Start);
                description_label.set_xalign(0.0);
                description_label.set_wrap(true);
                description_label.set_max_width_chars(60);
                description_label.add_css_class("dim-label");
                row_box.append(&description_label);
            }
            
            // Start-Datei
            let start_file_label = Label::new(Some(&format!("Start-Datei: {}", game_info.start_file)));
            start_file_label.set_halign(gtk4::Align::Start);
            start_file_label.set_xalign(0.0);
            start_file_label.add_css_class("dim-label");
            row_box.append(&start_file_label);
            
            // Start-Argumente (falls vorhanden)
            if let Some(ref args) = game_info.start_args {
                let args_label = Label::new(Some(&format!("Argumente: {}", args)));
                args_label.set_halign(gtk4::Align::Start);
                args_label.set_xalign(0.0);
                args_label.add_css_class("dim-label");
                row_box.append(&args_label);
            }
            
            // Pfad
            let path_label = Label::new(Some(&format!("Pfad: {}", path.display())));
            path_label.set_halign(gtk4::Align::Start);
            path_label.set_xalign(0.0);
            path_label.add_css_class("dim-label");
            row_box.append(&path_label);
            
            row.set_child(Some(&row_box));
            games_list.append(&row);
        }
    }
}

/// Erstellt Download-UI-Elemente für ein Spiel
fn create_download_ui(
    game_id: &str,
    active_downloads: &ActiveDownloads,
    download_request_tx: &tokio::sync::mpsc::UnboundedSender<deckdrop_network::network::discovery::DownloadRequest>,
    network_games_list_weak: &glib::WeakRef<ListBox>,
    network_games: &Rc<RefCell<HashMap<String, Vec<(String, deckdrop_network::network::games::NetworkGameInfo)>>>>,
) {
    let progress_bar = ProgressBar::new();
    progress_bar.set_show_text(true);
    progress_bar.set_fraction(0.0);
    
    let status_label = Label::new(Some("Initialisiere Download..."));
    status_label.set_halign(gtk4::Align::Start);
    status_label.set_xalign(0.0);
    
    let pause_button = Button::with_label("Pause");
    let resume_button = Button::with_label("Resume");
    resume_button.set_visible(false);
    
    let cancel_button = Button::with_label("Abbrechen");
    cancel_button.add_css_class("destructive-action");
    
    let button_box = GtkBox::new(Orientation::Horizontal, 6);
    button_box.append(&pause_button);
    button_box.append(&resume_button);
    button_box.append(&cancel_button);
    
    // Clone für Callbacks
    let game_id_clone = game_id.to_string();
    let active_downloads_clone = active_downloads.clone();
    let download_request_tx_clone = download_request_tx.clone();
    let network_games_list_weak_clone = network_games_list_weak.clone();
    
    // Pause-Button
    pause_button.connect_clicked(move |_| {
        if let Ok(manifest_path) = crate::synch::get_manifest_path(&game_id_clone) {
            if let Ok(mut manifest) = crate::synch::DownloadManifest::load(&manifest_path) {
                manifest.overall_status = crate::synch::DownloadStatus::Paused;
                let _ = manifest.save(&manifest_path);
                update_download_ui(&game_id_clone, &active_downloads_clone);
            }
        }
    });
    
    // Resume-Button
    let game_id_clone2 = game_id.to_string();
    let active_downloads_clone2 = active_downloads.clone();
    let download_request_tx_clone2 = download_request_tx.clone();
    let network_games_clone_for_resume = network_games.clone();
    resume_button.connect_clicked(move |_| {
        if let Ok(manifest_path) = crate::synch::get_manifest_path(&game_id_clone2) {
            if let Ok(mut manifest) = crate::synch::DownloadManifest::load(&manifest_path) {
                manifest.overall_status = crate::synch::DownloadStatus::Downloading;
                let _ = manifest.save(&manifest_path);
                
                // Hole verfügbare Peers aus network_games
                // Suche nach game_id in allen unique_keys
                let peer_ids: Vec<String> = {
                    let network_games_borrowed = network_games_clone_for_resume.borrow();
                    let mut peers = Vec::new();
                    for (_unique_key, peer_games) in network_games_borrowed.iter() {
                        if let Some((_, game_info)) = peer_games.first() {
                            if game_info.game_id == game_id_clone2 {
                                peers.extend(peer_games.iter().map(|(pid, _)| pid.clone()));
                                break;
                            }
                        }
                    }
                    peers
                };
                
                if !peer_ids.is_empty() {
                    let _ = crate::synch::request_missing_chunks(&game_id_clone2, &peer_ids, &download_request_tx_clone2);
                } else {
                    eprintln!("Keine verfügbaren Peers für Resume von Spiel {}", game_id_clone2);
                }
                
                update_download_ui(&game_id_clone2, &active_downloads_clone2);
            }
        }
    });
    
    // Cancel-Button
    let game_id_clone3 = game_id.to_string();
    let active_downloads_clone3 = active_downloads.clone();
    let network_games_list_weak_clone2 = network_games_list_weak.clone();
    let network_games_clone_for_cancel = network_games.clone();
    let download_request_tx_clone_for_cancel = download_request_tx.clone();
    cancel_button.connect_clicked(move |_| {
        if let Err(e) = crate::synch::cancel_game_download(&game_id_clone3) {
            eprintln!("Fehler beim Abbrechen des Downloads: {}", e);
        } else {
            active_downloads_clone3.borrow_mut().remove(&game_id_clone3);
            // Aktualisiere Liste
            if let Some(network_games_list) = network_games_list_weak_clone2.upgrade() {
                update_network_games_list(&network_games_list, &network_games_clone_for_cancel, &download_request_tx_clone_for_cancel, &active_downloads_clone3);
            }
        }
    });
    
    let download_ui = DownloadUI {
        progress_bar,
        status_label,
        pause_button,
        resume_button,
        cancel_button,
        button_box,
    };
    
    active_downloads.borrow_mut().insert(game_id.to_string(), download_ui);
}

/// Aktualisiert die Download-UI für ein Spiel
fn update_download_ui(game_id: &str, active_downloads: &ActiveDownloads) {
    if let Some(download_ui) = active_downloads.borrow().get(game_id) {
        if let Ok(manifest_path) = crate::synch::get_manifest_path(game_id) {
            if let Ok(manifest) = crate::synch::DownloadManifest::load(&manifest_path) {
                download_ui.progress_bar.set_fraction(manifest.progress.percentage / 100.0);
                download_ui.progress_bar.set_text(Some(&format!("{}%", manifest.progress.percentage as u32)));
                download_ui.status_label.set_text(&format!("{} von {} Chunks heruntergeladen", 
                    manifest.progress.downloaded_chunks,
                    manifest.progress.total_chunks));
                
                // Aktualisiere Button-Sichtbarkeit
                match manifest.overall_status {
                    crate::synch::DownloadStatus::Downloading => {
                        download_ui.pause_button.set_visible(true);
                        download_ui.resume_button.set_visible(false);
                        download_ui.cancel_button.set_visible(true);
                    }
                    crate::synch::DownloadStatus::Paused => {
                        download_ui.pause_button.set_visible(false);
                        download_ui.resume_button.set_visible(true);
                        download_ui.cancel_button.set_visible(true);
                    }
                    crate::synch::DownloadStatus::Complete => {
                        download_ui.pause_button.set_visible(false);
                        download_ui.resume_button.set_visible(false);
                        download_ui.cancel_button.set_visible(false);
                        download_ui.status_label.set_text("✓ Download abgeschlossen");
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Struktur für Download-UI-Elemente
#[derive(Clone)]
struct DownloadUI {
    progress_bar: ProgressBar,
    status_label: Label,
    pause_button: Button,
    resume_button: Button,
    cancel_button: Button,
    button_box: GtkBox,
}

/// Aktive Downloads tracken (game_id -> Download-UI)
type ActiveDownloads = Rc<RefCell<HashMap<String, DownloadUI>>>;

fn update_network_games_list(
    games_list: &ListBox, 
    network_games: &Rc<RefCell<HashMap<String, Vec<(String, deckdrop_network::network::games::NetworkGameInfo)>>>>,
    download_request_tx: &tokio::sync::mpsc::UnboundedSender<deckdrop_network::network::discovery::DownloadRequest>,
    active_downloads: &ActiveDownloads,
) {
    // Entferne alle vorhandenen Zeilen
    while let Some(row) = games_list.row_at_index(0) {
        games_list.remove(&row);
    }
    
    let games_map = network_games.borrow();
    
    println!("update_network_games_list: {} Spiele in der Map", games_map.len());
    eprintln!("update_network_games_list: {} Spiele in der Map", games_map.len());
    
    if games_map.is_empty() {
        let row = ListBoxRow::new();
        let label = Label::new(Some("Keine Spiele im Netzwerk gefunden. Warte auf Peers..."));
        label.set_halign(gtk4::Align::Center);
        label.add_css_class("dim-label");
        row.set_child(Some(&label));
        games_list.append(&row);
        return;
    }
    
    // Lade lokale Spiele und erstelle eine Set der game_ids
    let local_game_ids: std::collections::HashSet<String> = {
        let config = crate::config::Config::load();
        let mut ids = std::collections::HashSet::new();
        
        for game_path in &config.game_paths {
            // Prüfe, ob das Verzeichnis selbst ein Spiel enthält
            if check_game_config_exists(game_path) {
                if let Ok(game_info) = GameInfo::load_from_path(game_path) {
                    ids.insert(game_info.game_id);
                }
            }
            // Lade auch Spiele aus Unterverzeichnissen
            if game_path.is_dir() {
                let additional_games = load_games_from_directory(game_path);
                for (_path, game_info) in additional_games {
                    ids.insert(game_info.game_id);
                }
            }
        }
        
        ids
    };
    
    // Zeige alle Spiele, gruppiert nach game_id + version (unique_key)
    for (_unique_key, peer_games) in games_map.iter() {
        println!("Zeige Spiel: {} ({} Peers)", peer_games.first().map(|(_, g)| &g.name).unwrap_or(&"Unknown".to_string()), peer_games.len());
        eprintln!("Zeige Spiel: {} ({} Peers)", peer_games.first().map(|(_, g)| &g.name).unwrap_or(&"Unknown".to_string()), peer_games.len());
        // Nimm das erste Spiel als Referenz (alle sollten die gleichen Metadaten haben)
        if let Some((_, game_info)) = peer_games.first() {
            let row = ListBoxRow::new();
            let row_box = GtkBox::new(Orientation::Vertical, 6);
            row_box.set_margin_start(12);
            row_box.set_margin_end(12);
            row_box.set_margin_top(12);
            row_box.set_margin_bottom(12);
            
            // Name
            let name_label = Label::new(Some(&game_info.name));
            name_label.set_halign(gtk4::Align::Start);
            name_label.set_xalign(0.0);
            name_label.add_css_class("title-4");
            row_box.append(&name_label);
            
            // Version
            let version_label = Label::new(Some(&format!("Version: {}", game_info.version)));
            version_label.set_halign(gtk4::Align::Start);
            version_label.set_xalign(0.0);
            row_box.append(&version_label);
            
            // Anzahl Peers, die dieses Spiel haben (immer anzeigen)
            let peers_count = peer_games.len();
            let peers_label = Label::new(Some(&format!("Verfügbar bei {} Peer{}", 
                peers_count, 
                if peers_count == 1 { "" } else { "s" })));
            peers_label.set_halign(gtk4::Align::Start);
            peers_label.set_xalign(0.0);
            peers_label.add_css_class("dim-label");
            row_box.append(&peers_label);
            
            // Prüfe, ob das Spiel auch in der eigenen Bibliothek vorhanden ist
            if local_game_ids.contains(&game_info.game_id) {
                let local_label = Label::new(Some("✓ Auch in eigener Bibliothek vorhanden"));
                local_label.set_halign(gtk4::Align::Start);
                local_label.set_xalign(0.0);
                local_label.add_css_class("success");
                local_label.add_css_class("dim-label");
                row_box.append(&local_label);
            }
            
            // Beschreibung (falls vorhanden)
            if let Some(ref description) = game_info.description {
                let description_label = Label::new(Some(description));
                description_label.set_halign(gtk4::Align::Start);
                description_label.set_xalign(0.0);
                description_label.set_wrap(true);
                description_label.set_max_width_chars(60);
                description_label.add_css_class("dim-label");
                row_box.append(&description_label);
            }
            
            // Start-Datei
            let start_file_label = Label::new(Some(&format!("Start-Datei: {}", game_info.start_file)));
            start_file_label.set_halign(gtk4::Align::Start);
            start_file_label.set_xalign(0.0);
            start_file_label.add_css_class("dim-label");
            row_box.append(&start_file_label);
            
            // Prüfe ob Download bereits aktiv ist
            let game_id_for_download = game_info.game_id.clone();
            let active_downloads_clone = active_downloads.clone();
            let download_request_tx_clone_for_download = download_request_tx.clone();
            
            // Prüfe ob bereits ein Download für dieses Spiel existiert
            let has_download = active_downloads.borrow().contains_key(&game_id_for_download);
            
            if has_download {
                // Aktualisiere UI zuerst
                update_download_ui(&game_id_for_download, active_downloads);
                
                // Hole UI-Elemente
                if let Some(download_ui) = active_downloads.borrow().get(&game_id_for_download) {
                    row_box.append(&download_ui.progress_bar);
                    row_box.append(&download_ui.status_label);
                    row_box.append(&download_ui.button_box);
                }
            } else {
                // "Get this game" Button
                let download_button = Button::with_label("Get this game");
                download_button.add_css_class("suggested-action");
                
                // Clone für den Callback
                let game_id_clone = game_info.game_id.clone();
                let game_name_clone = game_info.name.clone();
                let peer_ids: Vec<String> = peer_games.iter().map(|(pid, _)| pid.clone()).collect();
                
                let download_request_tx_for_button = download_request_tx.clone();
                download_button.connect_clicked(move |_| {
                    println!("Download gestartet für Spiel: {} (ID: {})", game_name_clone, game_id_clone);
                    eprintln!("Download gestartet für Spiel: {} (ID: {})", game_name_clone, game_id_clone);
                    
                    // Wähle ersten verfügbaren Peer
                    if let Some(peer_id) = peer_ids.first() {
                        // Sende GameMetadataRequest
                        if let Err(e) = download_request_tx_for_button.send(
                            deckdrop_network::network::discovery::DownloadRequest::RequestGameMetadata {
                                peer_id: peer_id.clone(),
                                game_id: game_id_clone.clone(),
                            }
                        ) {
                            eprintln!("Fehler beim Senden von GameMetadataRequest: {}", e);
                        } else {
                            println!("GameMetadataRequest gesendet an Peer {} für Spiel {}", peer_id, game_id_clone);
                            eprintln!("GameMetadataRequest gesendet an Peer {} für Spiel {}", peer_id, game_id_clone);
                        }
                    } else {
                        eprintln!("Keine verfügbaren Peers für Download von Spiel {}", game_id_clone);
                    }
                });
                
                row_box.append(&download_button);
            }
            
            row.set_child(Some(&row_box));
            games_list.append(&row);
        }
    }
}

fn create_settings_tab(config: &Config) -> GtkBox {
    let box_ = GtkBox::new(Orientation::Vertical, 12);
    box_.set_margin_start(20);
    box_.set_margin_end(20);
    box_.set_margin_top(20);
    box_.set_margin_bottom(20);
    
    // Spielername
    let name_label = Label::new(Some("Spielername:"));
    name_label.set_halign(gtk4::Align::Start);
    box_.append(&name_label);
    
    let player_name_entry = GtkEntry::new();
    player_name_entry.set_text(&config.player_name);
    player_name_entry.set_hexpand(true);
    box_.append(&player_name_entry);
    
    // Download-Pfad
    let path_label = Label::new(Some("Download-Pfad:"));
    path_label.set_halign(gtk4::Align::Start);
    path_label.set_margin_top(12);
    box_.append(&path_label);
    
    let path_box = GtkBox::new(Orientation::Horizontal, 8);
    
    let download_path_entry = GtkEntry::new();
    download_path_entry.set_text(&config.download_path.to_string_lossy());
    download_path_entry.set_hexpand(true);
    path_box.append(&download_path_entry);
    
    let browse_button = Button::with_label("Durchsuchen...");
    browse_button.set_halign(gtk4::Align::End);
    path_box.append(&browse_button);
    
    box_.append(&path_box);
    
    // Speichern-Button
    let save_button = Button::with_label("Speichern");
    save_button.add_css_class("suggested-action");
    save_button.set_halign(gtk4::Align::End);
    save_button.set_margin_top(20);
    box_.append(&save_button);
    
    // Event-Handler für Browse-Button
    let download_path_entry_clone = download_path_entry.clone();
    browse_button.connect_clicked(move |_| {
        let dialog = gtk4::FileChooserDialog::builder()
            .title("Download-Pfad auswählen")
            .action(gtk4::FileChooserAction::SelectFolder)
            .build();
        
        let entry_clone = download_path_entry_clone.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk4::ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        entry_clone.set_text(&path.to_string_lossy());
                    }
                }
            }
            dialog.close();
        });
        
        dialog.show();
    });
    
    // Peer-ID anzeigen (nur lesend)
    let peer_id_label = Label::new(Some("Peer-ID:"));
    peer_id_label.set_halign(gtk4::Align::Start);
    box_.append(&peer_id_label);
    
    let peer_id_display = Label::new(config.peer_id.as_deref());
    peer_id_display.set_halign(gtk4::Align::Start);
    peer_id_display.set_xalign(0.0);
    peer_id_display.add_css_class("dim-label");
    peer_id_display.set_wrap(true);
    peer_id_display.set_selectable(true);
    box_.append(&peer_id_display);
    
    // Event-Handler für Speichern-Button
    let player_name_entry_clone = player_name_entry.clone();
    let download_path_entry_clone = download_path_entry.clone();
    let peer_id_to_save = config.peer_id.clone();
    save_button.connect_clicked(move |_| {
        let player_name = player_name_entry_clone.text().to_string();
        let download_path_str = download_path_entry_clone.text().to_string();
        let download_path = std::path::PathBuf::from(download_path_str);
        
        let config = Config {
            player_name,
            download_path,
            peer_id: peer_id_to_save.clone(),
            game_paths: Vec::new(), // Wird aus der gespeicherten Config geladen
        };
        
        if let Err(e) = config.save() {
            eprintln!("Fehler beim Speichern der Konfiguration: {}", e);
        } else {
            println!("Konfiguration gespeichert");
        }
    });
    
    box_
}

fn update_peers_list(peers_list: &ListBox, known_peers: &Rc<RefCell<HashMap<String, PeerInfo>>>) {
    // Entferne alle vorhandenen Zeilen
    while let Some(row) = peers_list.row_at_index(0) {
        peers_list.remove(&row);
    }
    
    // Füge alle bekannten Peers hinzu
    let peers = known_peers.borrow();
    for (id, peer_info) in peers.iter() {
        let row = ListBoxRow::new();
        let row_box = GtkBox::new(Orientation::Vertical, 6);
        row_box.set_margin_start(12);
        row_box.set_margin_end(12);
        row_box.set_margin_top(12);
        row_box.set_margin_bottom(12);
        
        // Spielername prominent anzeigen (falls vorhanden)
        if let Some(player_name) = &peer_info.player_name {
            let name_label = Label::new(Some(player_name));
            name_label.set_halign(gtk4::Align::Start);
            name_label.set_xalign(0.0);
            name_label.add_css_class("title-4");
            row_box.append(&name_label);
        } else {
            // Fallback: Zeige Peer ID als Titel
            let id_short = if id.len() > 20 {
                format!("{}...", &id[..20])
            } else {
                id.clone()
            };
            let id_label = Label::new(Some(&format!("Peer: {}", id_short)));
            id_label.set_halign(gtk4::Align::Start);
            id_label.set_xalign(0.0);
            id_label.add_css_class("title-4");
            row_box.append(&id_label);
        }
        
        // Anzahl Spiele anzeigen (falls vorhanden)
        if let Some(games_count) = peer_info.games_count {
            let games_label = Label::new(Some(&format!("{} Spiele", games_count)));
            games_label.set_halign(gtk4::Align::Start);
            games_label.set_xalign(0.0);
            row_box.append(&games_label);
        }
        
        // Peer ID (kürzer anzeigen, nur wenn Spielername vorhanden)
        if peer_info.player_name.is_some() {
            let id_short = if id.len() > 20 {
                format!("{}...", &id[..20])
            } else {
                id.clone()
            };
            let id_label = Label::new(Some(&format!("ID: {}", id_short)));
            id_label.set_halign(gtk4::Align::Start);
            id_label.set_xalign(0.0);
            id_label.add_css_class("dim-label");
            row_box.append(&id_label);
        }
        
        // Adresse
        if let Some(addr) = &peer_info.addr {
            let addr_label = Label::new(Some(&format!("Adresse: {}", addr)));
            addr_label.set_halign(gtk4::Align::Start);
            addr_label.set_xalign(0.0);
            addr_label.add_css_class("dim-label");
            row_box.append(&addr_label);
        }
        
        row.set_child(Some(&row_box));
        peers_list.append(&row);
    }
    
    if peers.is_empty() {
        let row = ListBoxRow::new();
        let label = Label::new(Some("Keine Peers gefunden. Warte auf Peers..."));
        label.set_halign(gtk4::Align::Center);
        label.add_css_class("dim-label");
        row.set_child(Some(&label));
        peers_list.append(&row);
    }
}
