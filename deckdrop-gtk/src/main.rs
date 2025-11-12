mod config;
mod settings;

use deckdrop_network::network::discovery::{start_discovery, DiscoveryEvent};
use deckdrop_network::network::peer::PeerInfo;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Label, Button, Box as GtkBox, Orientation, Separator, Stack, StackSwitcher, ScrolledWindow, ListBox, ListBoxRow, Entry as GtkEntry};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::thread;
use crate::config::Config;

fn main() {
    // GTK Anwendung
    let app = Application::builder()
        .application_id("com.deckdrop.gtk")
        .build();

    app.connect_activate(|app| {
        build_ui(app);
    });

    app.run();
}

fn build_ui(app: &Application) {
    // Lade Konfiguration beim Start
    let config = Config::load();
    println!("Geladene Konfiguration: Spielername = {}, Spiele-Pfad = {}", 
             config.player_name, config.games_path.display());

    // Hauptcontainer
    let main_box = GtkBox::new(Orientation::Vertical, 0);
    
    // Stack für Tabs
    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(200);
    
    // Tab 1: Meine Spiele
    let my_games_box = create_my_games_tab(&config);
    stack.add_titled(&my_games_box, Some("my-games"), "Meine Spiele");
    
    // Tab 2: Spiele im Netzwerk
    let network_games_box = create_network_games_tab();
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

    let window = ApplicationWindow::builder()
        .application(app)
        .title("DeckDrop GTK")
        .default_width(800)
        .default_height(600)
        .child(&main_box)
        .build();

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
    
    thread::spawn(move || {
        let rt = Runtime::new().expect("Failed to create Tokio runtime");
        
        // Network Discovery Task
        rt.spawn(async move {
            let _handle = start_discovery(event_tx.clone(), Some(player_name.clone()), games_count).await;

            while let Some(event) = event_rx.recv().await {
                // Events in den GTK Thread schicken
                let _ = glib_tx_clone.send(event).await;
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
    
    main_context.spawn_local_with_priority(glib::Priority::default(), async move {
        while let Ok(event) = glib_rx.recv().await {
            // Upgrade weak references
            let Some(peers_list) = peers_list_weak.upgrade() else { break };
            let Some(status_label) = status_label_weak.upgrade() else { break };
            
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
                    }
                    let peer_count = known_peers_clone.borrow().len();
                    status_label.set_text(&format!("Status: Online • Peers: {}", peer_count));
                }
            }
        }
    });
}

fn create_my_games_tab(config: &Config) -> GtkBox {
    let box_ = GtkBox::new(Orientation::Vertical, 12);
    box_.set_margin_start(20);
    box_.set_margin_end(20);
    box_.set_margin_top(20);
    box_.set_margin_bottom(20);
    
    let label = Label::new(Some(&format!("Spiele-Pfad: {}", config.games_path.display())));
    label.set_halign(gtk4::Align::Start);
    box_.append(&label);
    
    let info_label = Label::new(Some("Hier werden deine lokalen Spiele angezeigt.\n\nNoch nicht implementiert."));
    info_label.set_halign(gtk4::Align::Start);
    info_label.set_valign(gtk4::Align::Start);
    info_label.set_hexpand(true);
    info_label.set_vexpand(true);
    box_.append(&info_label);
    
    box_
}

fn create_network_games_tab() -> GtkBox {
    let box_ = GtkBox::new(Orientation::Vertical, 12);
    box_.set_margin_start(20);
    box_.set_margin_end(20);
    box_.set_margin_top(20);
    box_.set_margin_bottom(20);
    
    let info_label = Label::new(Some("Hier werden Spiele angezeigt, die im Netzwerk verfügbar sind.\n\nNoch nicht implementiert."));
    info_label.set_halign(gtk4::Align::Start);
    info_label.set_valign(gtk4::Align::Start);
    info_label.set_hexpand(true);
    info_label.set_vexpand(true);
    box_.append(&info_label);
    
    box_
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
    
    // Spiele-Pfad
    let path_label = Label::new(Some("Spiele-Pfad:"));
    path_label.set_halign(gtk4::Align::Start);
    path_label.set_margin_top(12);
    box_.append(&path_label);
    
    let path_box = GtkBox::new(Orientation::Horizontal, 8);
    
    let games_path_entry = GtkEntry::new();
    games_path_entry.set_text(&config.games_path.to_string_lossy());
    games_path_entry.set_hexpand(true);
    path_box.append(&games_path_entry);
    
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
    let games_path_entry_clone = games_path_entry.clone();
    browse_button.connect_clicked(move |_| {
        let dialog = gtk4::FileChooserDialog::builder()
            .title("Spiele-Pfad auswählen")
            .action(gtk4::FileChooserAction::SelectFolder)
            .build();
        
        let entry_clone = games_path_entry_clone.clone();
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
    
    // Event-Handler für Speichern-Button
    let player_name_entry_clone = player_name_entry.clone();
    let games_path_entry_clone = games_path_entry.clone();
    save_button.connect_clicked(move |_| {
        let player_name = player_name_entry_clone.text().to_string();
        let games_path_str = games_path_entry_clone.text().to_string();
        let games_path = std::path::PathBuf::from(games_path_str);
        
        let config = Config {
            player_name,
            games_path,
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
