//! deckdrop-ui - Iced-basierte Benutzeroberfläche für DeckDrop

mod app;
mod network_bridge;

use app::{DeckDropApp, Message};
use iced::application;
use std::sync::Arc;
use tokio::sync::mpsc;

fn main() -> iced::Result {
    // Starte Network-Thread
    let network_bridge = network_bridge::NetworkBridge::start();
    
    // Wandle Receiver in Arc<Mutex<...>> um und setze global
    let network_event_rx = Arc::new(std::sync::Mutex::new(network_bridge.event_rx));
    network_bridge::set_network_event_rx(network_event_rx);
    
    // Setze Download Request Sender global
    network_bridge::set_download_request_tx(network_bridge.download_request_tx);
    
    application(
        "DeckDrop",
        DeckDropApp::update,
        DeckDropApp::view,
    )
    .theme(|_state: &DeckDropApp| iced::Theme::Dark)
    .subscription(move |_state: &DeckDropApp| {
        // Periodischer Tick für Event-Polling
        use iced::Subscription;
        
        // Erstelle einen Stream, der periodisch Ticks sendet
        use iced::futures::StreamExt;
        use iced::futures::stream;
        
        Subscription::run(move || {
            stream::unfold((), move |_| {
                async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    Some((Message::Tick, ()))
                }
            })
        })
    })
    .window(iced::window::Settings {
        size: iced::Size::new(1000.0, 700.0),
        min_size: Some(iced::Size::new(800.0, 600.0)),
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

