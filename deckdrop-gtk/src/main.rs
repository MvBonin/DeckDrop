mod config;
mod settings;

use deckdrop_network::network::discovery::{start_discovery, DiscoveryEvent};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Label, Button, Box as GtkBox, Orientation};
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use glib::clone;
use crate::config::Config;
use crate::settings::SettingsWindow;

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
    let main_box = GtkBox::new(Orientation::Vertical, 12);
    main_box.set_margin_start(20);
    main_box.set_margin_end(20);
    main_box.set_margin_top(20);
    main_box.set_margin_bottom(20);

    // Label, das wir updaten, wenn ein Peer entdeckt wird
    let label = Label::new(Some("Waiting for peers..."));
    label.set_hexpand(true);
    label.set_vexpand(true);
    main_box.append(&label);

    // Settings-Button
    let settings_button = Button::with_label("Einstellungen");
    settings_button.set_halign(gtk4::Align::End);
    main_box.append(&settings_button);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("DeckDrop GTK")
        .default_width(600)
        .default_height(400)
        .child(&main_box)
        .build();

    // Settings-Fenster öffnen
    let window_clone = window.clone();
    settings_button.connect_clicked(move |_| {
        let settings = SettingsWindow::new(Some(&window_clone));
        settings.show();
    });

    window.show();

    // Channel zwischen Network (Tokio) und GTK
    let (event_tx, mut event_rx) = mpsc::channel::<DiscoveryEvent>(32);

    // Async channel für GTK Main Context
    let (glib_tx, glib_rx) = async_channel::unbounded::<DiscoveryEvent>();

    // Tokio Runtime im Hintergrund
    let rt = Runtime::new().expect("Failed to create Tokio runtime");

    // Network Discovery Task
    let glib_tx_clone = glib_tx.clone();
    rt.spawn(async move {
        let _handle = start_discovery(event_tx.clone()).await;

        while let Some(event) = event_rx.recv().await {
            // Events in den GTK Thread schicken
            let _ = glib_tx_clone.send(event).await;
        }
    });

    // Events im GTK Thread verarbeiten
    let main_context = glib::MainContext::default();
    main_context.spawn_local_with_priority(glib::Priority::default(), clone!(@weak label => async move {
        while let Ok(event) = glib_rx.recv().await {
            match event {
                DiscoveryEvent::PeerFound(peer) => {
                    label.set_text(&format!("Peer found: {}", peer.id));
                }
                DiscoveryEvent::PeerLost(peer_id) => {
                    label.set_text(&format!("Peer lost: {}", peer_id));
                }
            }
        }
    }));
}
