mod config;
mod settings;
mod game;

use deckdrop_network::network::discovery::{start_discovery, DiscoveryEvent};
use deckdrop_network::network::peer::PeerInfo;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Label, Button, Box as GtkBox, Orientation, Separator, Stack, StackSwitcher, ScrolledWindow, ListBox, ListBoxRow, Entry as GtkEntry, MessageDialog, MessageType, ButtonsType, Window, FileChooserDialog, FileChooserAction, ResponseType, TextView};
use crate::game::{GameInfo, check_game_config_exists, load_games_from_directory};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::thread;
use std::env;
use crate::config::Config;

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
            let mut config = Config::load();
            // Lösche alte Keypair-Datei, falls vorhanden
            if let Some(keypair_path) = Config::keypair_path() {
                if keypair_path.exists() {
                    if let Err(e) = std::fs::remove_file(&keypair_path) {
                        eprintln!("Warnung: Konnte alte Keypair-Datei nicht löschen: {}", e);
                    } else {
                        println!("Alte Peer-ID gelöscht");
                    }
                }
            }
            // Generiere neue Peer-ID
            if let Err(e) = config.generate_and_save_peer_id() {
                eprintln!("Fehler beim Generieren der neuen Peer-ID: {}", e);
                app.quit();
                return;
            }
            println!("Neue Peer-ID generiert: {:?}", config.peer_id);
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

    // Async channel für GTK Main Context
    let (glib_tx, glib_rx) = async_channel::unbounded::<DiscoveryEvent>();

    // Tokio Runtime in einem separaten Thread starten, damit sie am Leben bleibt
    let glib_tx_clone = glib_tx.clone();
    let player_name = config.player_name.clone();
    // TODO: Später die tatsächliche Anzahl der Spiele berechnen
    let games_count = Some(0u32); // Platzhalter für jetzt
    
    // Lade Keypair für persistente Peer-ID
    let keypair = crate::config::Config::load_keypair();
    
    thread::spawn(move || {
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        
        // Network Discovery Task
        rt.spawn(async move {
            let _handle = start_discovery(event_tx.clone(), Some(player_name.clone()), games_count, keypair).await;
            println!("Discovery gestartet, warte auf Events...");
            eprintln!("Discovery gestartet, warte auf Events...");

            while let Some(event) = event_rx.recv().await {
                println!("Event empfangen im Tokio Thread: {:?}", event);
                eprintln!("Event empfangen im Tokio Thread: {:?}", event);
                // Events in den GTK Thread schicken
                if let Err(e) = glib_tx_clone.send(event).await {
                    eprintln!("Fehler beim Senden des Events an GTK Thread: {}", e);
                } else {
                    println!("Event erfolgreich an GTK Thread gesendet");
                    eprintln!("Event erfolgreich an GTK Thread gesendet");
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
    
    main_context.spawn_local_with_priority(glib::Priority::default(), async move {
        println!("GTK Event-Handler gestartet, warte auf Events...");
        eprintln!("GTK Event-Handler gestartet, warte auf Events...");
        
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
                        let mut games = network_games_clone.borrow_mut();
                        games.retain(|_, peer_games| {
                            peer_games.retain(|(p_id, _)| p_id != &peer_id);
                            !peer_games.is_empty()
                        });
                        
                        if let Some(network_games_list) = network_games_list_weak.upgrade() {
                            update_network_games_list(&network_games_list, &network_games_clone);
                        }
                    }
                    let peer_count = known_peers_clone.borrow().len();
                    status_label.set_text(&format!("Status: Online • Peers: {}", peer_count));
                }
                DiscoveryEvent::GamesListReceived { peer_id, games } => {
                    println!("Spiele-Liste erhalten von {}: {} Spiele", peer_id, games.len());
                    
                    // Aktualisiere die Netzwerk-Spiele-Liste
                    let mut network_games_map = network_games_clone.borrow_mut();
                    for game in games {
                        network_games_map
                            .entry(game.game_id.clone())
                            .or_insert_with(Vec::new)
                            .push((peer_id.clone(), game));
                    }
                    
                    if let Some(network_games_list) = network_games_list_weak.upgrade() {
                        update_network_games_list(&network_games_list, &network_games_clone);
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
    browse_button.connect_clicked(move |_| {
        let file_dialog = FileChooserDialog::builder()
            .title("Spielverzeichnis auswählen")
            .action(FileChooserAction::SelectFolder)
            .transient_for(&dialog_clone)
            .build();
        
        let download_path_clone2 = download_path_clone.clone();
        let games_list_clone2 = games_list_clone.clone();
        let dialog_clone2 = dialog_clone.clone();
        file_dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        // Prüfe ob bereits eine valide TOML existiert
                        if check_game_config_exists(&path) {
                            if GameInfo::load_from_path(&path).is_ok() {
                                // Valide TOML gefunden - Spiel direkt hinzufügen ohne Dialog
                                // Die Peer-ID bleibt die vom Ersteller (bereits in der TOML)
                                // Das Spiel wird automatisch zur Liste hinzugefügt
                                dialog_clone2.close();
                                update_games_list(&games_list_clone2, &download_path_clone2);
                                return;
                            }
                        }
                        // Keine valide TOML - Pfad setzen und Dialog bleibt offen
                        // (Der Pfad wird im path_entry gesetzt, aber das ist nicht mehr nötig,
                        // da wir den Dialog schließen wenn TOML vorhanden ist)
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
        
        // Erstelle GameInfo mit Peer-ID
        let game_info = GameInfo {
            game_id: crate::game::generate_game_id(), // Generiere neue game_id
            name: name.clone(),
            version: if version.is_empty() { "1.0".to_string() } else { version },
            start_file,
            start_args: if start_args.is_empty() { None } else { Some(start_args) },
            description: if description.trim().is_empty() { None } else { Some(description.trim().to_string()) },
            creator_peer_id: peer_id.clone(),
        };
        
        // Speichere TOML (überschreibt vorhandene)
        match game_info.save_to_path(&game_path) {
            Ok(_) => {
                dialog_clone.close();
                update_games_list(&games_list_clone, &download_path_clone);
            }
            Err(e) => {
                show_error_dialog(&dialog_clone, &format!("Fehler beim Speichern: {}", e));
            }
        }
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

fn update_games_list(games_list: &ListBox, download_path: &std::path::PathBuf) {
    // Entferne alle vorhandenen Zeilen
    while let Some(row) = games_list.row_at_index(0) {
        games_list.remove(&row);
    }
    
    // Lade Spiele aus dem Verzeichnis
    let games = load_games_from_directory(download_path);
    
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

fn update_network_games_list(games_list: &ListBox, network_games: &Rc<RefCell<HashMap<String, Vec<(String, deckdrop_network::network::games::NetworkGameInfo)>>>>) {
    // Entferne alle vorhandenen Zeilen
    while let Some(row) = games_list.row_at_index(0) {
        games_list.remove(&row);
    }
    
    let games_map = network_games.borrow();
    
    if games_map.is_empty() {
        let row = ListBoxRow::new();
        let label = Label::new(Some("Keine Spiele im Netzwerk gefunden. Warte auf Peers..."));
        label.set_halign(gtk4::Align::Center);
        label.add_css_class("dim-label");
        row.set_child(Some(&label));
        games_list.append(&row);
        return;
    }
    
    // Zeige alle Spiele, gruppiert nach game_id
    for (_game_id, peer_games) in games_map.iter() {
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
            
            // Anzahl Peers, die dieses Spiel haben
            if peer_games.len() > 1 {
                let peers_label = Label::new(Some(&format!("Verfügbar bei {} Peers", peer_games.len())));
                peers_label.set_halign(gtk4::Align::Start);
                peers_label.set_xalign(0.0);
                peers_label.add_css_class("dim-label");
                row_box.append(&peers_label);
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
