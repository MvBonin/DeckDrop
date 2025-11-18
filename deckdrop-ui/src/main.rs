//! deckdrop-ui - Iced-basierte Benutzeroberfläche für DeckDrop

mod app;
mod network_bridge;
mod window_control;

use app::{DeckDropApp, Message};
use iced::application;
use std::sync::Arc;
use tokio::sync::mpsc;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;

/// Globaler Window-Operation-Receiver (für einfachen Zugriff)
static WINDOW_OP_RX: std::sync::OnceLock<Arc<std::sync::Mutex<std_mpsc::Receiver<Message>>>> = std::sync::OnceLock::new();

/// Setzt den globalen Window-Operation-Receiver
fn set_window_op_rx(rx: Arc<std::sync::Mutex<std_mpsc::Receiver<Message>>>) {
    let _ = WINDOW_OP_RX.set(rx);
}

/// Gibt den globalen Window-Operation-Receiver zurück
pub fn get_window_op_rx() -> Option<Arc<std::sync::Mutex<std_mpsc::Receiver<Message>>>> {
    WINDOW_OP_RX.get().cloned()
}

fn main() -> iced::Result {
    // Prüfe Command-Line-Argumente
    let args: Vec<String> = env::args().collect();
    let random_id = args.iter().any(|arg| arg == "--random-id" || arg == "-random-id");
    let background = args.iter().any(|arg| arg == "--background" || arg == "-background" || arg == "--minimized" || arg == "-minimized");
    
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
        deckdrop_core::Config::set_peer_id_subdir(&peer_id_str);
        
        // Erstelle eine neue Konfiguration mit Standardwerten
        let mut config = deckdrop_core::Config::default();
        config.peer_id = Some(peer_id_str.clone());
        
        // Speichere die Keypair-Datei im neuen Unterordner
        if let Some(keypair_path) = deckdrop_core::Config::keypair_path() {
            if let Some(parent) = keypair_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("Fehler beim Erstellen des Verzeichnisses: {}", e);
                }
            }
            match keypair.to_protobuf_encoding() {
                Ok(bytes) => {
                    if let Err(e) = std::fs::write(&keypair_path, bytes) {
                        eprintln!("Fehler beim Speichern der Keypair: {}", e);
                        eprintln!("Bitte starten Sie die Anwendung erneut.");
                        std::process::exit(1);
                    }
                    println!("Neue Peer-ID generiert und gespeichert: {}", peer_id);
                }
                Err(e) => {
                    eprintln!("Fehler beim Serialisieren der Keypair: {}", e);
                    eprintln!("Bitte starten Sie die Anwendung erneut.");
                    std::process::exit(1);
                }
            }
        }
        
        // Speichere die neue Konfiguration (mit Standardwerten, leere Spieleliste)
        if let Err(e) = config.save() {
            eprintln!("Fehler beim Speichern der Konfiguration: {}", e);
            eprintln!("Bitte starten Sie die Anwendung erneut.");
            std::process::exit(1);
        }
        
        println!("Neue Konfiguration mit Peer-ID {} erstellt", peer_id);
    }
    
    // Starte Network-Thread
    let network_bridge = network_bridge::NetworkBridge::start();
    
    // Wandle Receiver in Arc<Mutex<...>> um und setze global
    let network_event_rx = Arc::new(std::sync::Mutex::new(network_bridge.event_rx));
    network_bridge::set_network_event_rx(network_event_rx);
    
    // Setze Download Request Sender global
    network_bridge::set_download_request_tx(network_bridge.download_request_tx);
    
    // Setze Metadata Update Sender global
    network_bridge::set_metadata_update_tx(network_bridge.metadata_update_tx);
    
    // Erstelle Channel für Window-Operationen vom System-Tray
    let (window_op_tx, window_op_rx) = std_mpsc::channel::<Message>();
    let window_op_rx = Arc::new(std::sync::Mutex::new(window_op_rx));
    
    // Setze globalen Window-Operation-Receiver (für Zugriff in app.rs)
    set_window_op_rx(window_op_rx.clone());
    
    // Initialisiere System-Tray-Icon
    if let Err(e) = init_system_tray(window_op_tx) {
        eprintln!("Warnung: System-Tray-Icon konnte nicht initialisiert werden: {}", e);
    }
    
    application(
        "DeckDrop",
        DeckDropApp::update,
        DeckDropApp::view,
    )
    .theme(|_state: &DeckDropApp| iced::Theme::Dark)
    .subscription(|_state: &DeckDropApp| {
        // Periodischer Tick für Event-Polling (alle 100ms)
        // Window-Operationen werden über den Tick-Mechanismus in app.rs abgefragt
        use iced::Subscription;
        use iced::futures::StreamExt;
        use iced::futures::stream;
        use std::time::Duration;
        use futures_timer::Delay;
        
        Subscription::run(move || {
            stream::unfold(
                (),
                move |_| async move {
                    Delay::new(Duration::from_millis(100)).await;
                    Some((Message::Tick, ()))
                }
            )
        })
    })
    .window(iced::window::Settings {
        size: iced::Size::new(800.0, 600.0),
        min_size: Some(iced::Size::new(640.0, 480.0)),
        visible: !background, // Fenster verstecken, wenn --background gesetzt ist
        ..Default::default()
    })
    .run()
}

/// Initialisiert das System-Tray-Icon
fn init_system_tray(window_op_tx: std_mpsc::Sender<Message>) -> Result<(), Box<dyn std::error::Error>> {
    use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem}};
    use image::ImageReader;
    
    // Lade Icon aus assets
    let icon_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("assets").join("deckdrop.png");
    
    // Lade Icon
    let icon = if icon_path.exists() {
        // Lade PNG mit image crate
        let img = ImageReader::open(&icon_path)?.decode()?;
        let rgba = img.to_rgba8();
        let width = rgba.width();
        let height = rgba.height();
        tray_icon::Icon::from_rgba(rgba.into_raw(), width, height)?
    } else {
        // Fallback: Erstelle ein einfaches Icon
        eprintln!("Warnung: Icon nicht gefunden unter {:?}, verwende Standard-Icon", icon_path);
        // Erstelle ein einfaches 16x16 Icon
        let rgba = vec![255u8; 16 * 16 * 4];
        tray_icon::Icon::from_rgba(rgba, 16, 16)?
    };
    
    // Erstelle Menü
    let mut menu = Menu::new();
    
    let show_item = MenuItem::new("Fenster anzeigen", true, None);
    let hide_item = MenuItem::new("Fenster verstecken", true, None);
    let quit_item = MenuItem::new("Beenden", true, None);
    
    // Speichere IDs vor dem move
    let show_id = show_item.id();
    let hide_id = hide_item.id();
    let quit_id = quit_item.id();
    
    menu.append(&show_item)?;
    menu.append(&hide_item)?;
    menu.append(&quit_item)?;
    
    // Erstelle Tray-Icon
    let _tray_icon = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("DeckDrop")
        .with_menu(Box::new(menu))
        .build()?;
    
    // Starte Thread für Tray-Events
    // Die IDs werden vor dem move kopiert
    let show_id_clone = show_id.clone();
    let hide_id_clone = hide_id.clone();
    let quit_id_clone = quit_id.clone();
    std::thread::spawn(move || {
        use tray_icon::{TrayIconEvent, menu::MenuEvent};
        
        loop {
            // Prüfe auf Tray-Icon-Events
            if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                match event {
                    TrayIconEvent::Click { .. } => {
                        // Bei Klick: Fenster anzeigen/verstecken umschalten
                        let _ = window_op_tx.send(Message::ShowWindow);
                    }
                    _ => {}
                }
            }
            
            // Prüfe auf Menü-Events
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == show_id_clone {
                    let _ = window_op_tx.send(Message::ShowWindow);
                } else if event.id == hide_id_clone {
                    let _ = window_op_tx.send(Message::HideWindow);
                } else if event.id == quit_id_clone {
                    let _ = window_op_tx.send(Message::Quit);
                }
            }
            
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    
    Ok(())
}

