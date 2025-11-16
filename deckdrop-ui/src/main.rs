//! deckdrop-ui - Iced-basierte Benutzeroberfläche für DeckDrop

mod app;

use app::{DeckDropApp, Message};
use iced::application;

fn main() -> iced::Result {
    application(
        "DeckDrop",
        DeckDropApp::update,
        DeckDropApp::view,
    )
    .theme(|_state: &DeckDropApp| iced::Theme::Dark)
    .window(iced::window::Settings {
        size: iced::Size::new(1000.0, 700.0),
        min_size: Some(iced::Size::new(800.0, 600.0)),
        ..Default::default()
    })
    .run()
}

