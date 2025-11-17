//! deckdrop-ui - Iced-basierte Benutzeroberfläche für DeckDrop

mod app;
mod network_bridge;

use app::{DeckDropApp, Message};
use iced::application;
use std::sync::Arc;
use tokio::sync::mpsc;
use std::env;

fn main() -> iced::Result {
    // Prüfe Command-Line-Argumente
    let args: Vec<String> = env::args().collect();
    let random_id = args.iter().any(|arg| arg == "--random-id" || arg == "-random-id");
    
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
    
    application(
        "DeckDrop",
        DeckDropApp::update,
        DeckDropApp::view,
    )
    .theme(|_state: &DeckDropApp| iced::Theme::Dark)
    .subscription(|_state: &DeckDropApp| {
        // Periodischer Tick für Event-Polling (alle 100ms)
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
        ..Default::default()
    })
    .run()
}

/// Subscription für Network-Events
/// 
/// Da die Iced Subscription API kompliziert ist, verwenden wir einen einfacheren Ansatz:
/// Events werden über den Tick-Mechanismus abgefragt (siehe app.rs update für Message::Tick)
fn network_subscription(
    _event_rx: Arc<std::sync::Mutex<mpsc::Receiver<deckdrop_network::network::discovery::DiscoveryEvent>>>,
) -> iced::Subscription<Message> {
    // Leere Subscription - Events werden über Tick abgefragt
    iced::Subscription::none()
}

