//! deckdrop-ui - Iced-basierte Benutzeroberfläche für DeckDrop

mod app;
mod network_bridge;
mod window_control;

use app::{DeckDropApp, Message};
use iced::application;
use std::sync::Arc;
use std::env;
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

/// Globaler Download-Preparation-Message-Sender (für Threads)
static DOWNLOAD_PREP_TX: std::sync::OnceLock<std_mpsc::Sender<Message>> = std::sync::OnceLock::new();

/// Setzt den globalen Download-Preparation-Message-Sender
fn set_download_prep_tx(tx: std_mpsc::Sender<Message>) {
    let _ = DOWNLOAD_PREP_TX.set(tx);
}

/// Gibt den globalen Download-Preparation-Message-Sender zurück
pub fn get_download_prep_tx() -> Option<std_mpsc::Sender<Message>> {
    DOWNLOAD_PREP_TX.get().cloned()
}

/// Globaler Download-Preparation-Message-Receiver (für Polling)
static DOWNLOAD_PREP_RX: std::sync::OnceLock<Arc<std::sync::Mutex<std_mpsc::Receiver<Message>>>> = std::sync::OnceLock::new();

/// Setzt den globalen Download-Preparation-Message-Receiver
fn set_download_prep_rx(rx: Arc<std::sync::Mutex<std_mpsc::Receiver<Message>>>) {
    let _ = DOWNLOAD_PREP_RX.set(rx);
}

/// Gibt den globalen Download-Preparation-Message-Receiver zurück
pub fn get_download_prep_rx() -> Option<Arc<std::sync::Mutex<std_mpsc::Receiver<Message>>>> {
    DOWNLOAD_PREP_RX.get().cloned()
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
    let (_window_op_tx, window_op_rx) = std_mpsc::channel::<Message>();
    let window_op_rx = Arc::new(std::sync::Mutex::new(window_op_rx));
    
    // Setze globalen Window-Operation-Receiver (für Zugriff in app.rs)
    set_window_op_rx(window_op_rx.clone());
    
    // Erstelle Channel für Download-Preparation-Messages (vom Thread zum UI-Thread)
    let (download_prep_tx, download_prep_rx) = std_mpsc::channel::<Message>();
    set_download_prep_tx(download_prep_tx);
    set_download_prep_rx(Arc::new(std::sync::Mutex::new(download_prep_rx)));
    eprintln!("Download-Preparation-Channel initialisiert");
    
    // System-Tray-Icon ist deaktiviert (tray-icon Abhängigkeit entfernt, um libxdo zu vermeiden)
    // Die Anwendung funktioniert weiterhin ohne Tray-Icon
    // window_op_tx wird nicht mehr verwendet, aber window_op_rx wird noch für andere Zwecke benötigt
    eprintln!("Info: System-Tray-Icon ist deaktiviert");
    
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

// System-Tray-Icon-Funktionalität wurde entfernt, um die libxdo-Abhängigkeit zu vermeiden
// Die init_system_tray Funktion wurde entfernt, da tray-icon nicht mehr verwendet wird

