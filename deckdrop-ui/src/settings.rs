use crate::config::Config;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Button, Entry, FileChooserDialog, FileChooserAction, 
           ResponseType, Label, Box as GtkBox, Orientation, Align};

#[allow(dead_code)]
pub struct SettingsWindow {
    window: ApplicationWindow,
    player_name_entry: Entry,
    download_path_entry: Entry,
}

#[allow(dead_code)]
impl SettingsWindow {
    pub fn new(parent: Option<&ApplicationWindow>) -> Self {
        let window = {
            let mut builder = ApplicationWindow::builder()
                .title("Einstellungen")
                .default_width(500)
                .default_height(300)
                .modal(true);
            if let Some(parent) = parent {
                builder = builder.transient_for(parent);
            }
            builder.build()
        };

        // Lade aktuelle Konfiguration
        let config = Config::load();

        // Hauptcontainer
        let main_box = GtkBox::new(Orientation::Vertical, 12);
        main_box.set_margin_start(20);
        main_box.set_margin_end(20);
        main_box.set_margin_top(20);
        main_box.set_margin_bottom(20);

        // Spielername
        let name_label = Label::new(Some("Spielername:"));
        name_label.set_halign(Align::Start);
        main_box.append(&name_label);

        let player_name_entry = Entry::new();
        player_name_entry.set_text(&config.player_name);
        player_name_entry.set_hexpand(true);
        main_box.append(&player_name_entry);

        // Download-Pfad
        let path_label = Label::new(Some("Download-Pfad:"));
        path_label.set_halign(Align::Start);
        main_box.append(&path_label);

        let path_box = GtkBox::new(Orientation::Horizontal, 8);
        
        let download_path_entry = Entry::new();
        download_path_entry.set_text(&config.download_path.to_string_lossy());
        download_path_entry.set_hexpand(true);
        path_box.append(&download_path_entry);

        let browse_button = Button::with_label("Durchsuchen...");
        browse_button.set_halign(Align::End);
        path_box.append(&browse_button);

        main_box.append(&path_box);

        // Button-Box
        let button_box = GtkBox::new(Orientation::Horizontal, 8);
        button_box.set_halign(Align::End);
        button_box.set_margin_top(20);

        let cancel_button = Button::with_label("Abbrechen");
        let save_button = Button::with_label("Speichern");
        save_button.add_css_class("suggested-action");

        button_box.append(&cancel_button);
        button_box.append(&save_button);

        main_box.append(&button_box);

        window.set_child(Some(&main_box));

        let settings = Self {
            window,
            player_name_entry,
            download_path_entry,
        };

        // Event-Handler
        let settings_clone = settings.clone_for_events();
        browse_button.connect_clicked(move |_| {
            settings_clone.browse_for_path();
        });

        let settings_clone = settings.clone_for_events();
        cancel_button.connect_clicked(move |_| {
            settings_clone.window.close();
        });

        let settings_clone = settings.clone_for_events();
        save_button.connect_clicked(move |_| {
            settings_clone.save_and_close();
        });

        settings
    }

    fn clone_for_events(&self) -> SettingsWindowClone {
        SettingsWindowClone {
            player_name_entry: self.player_name_entry.clone(),
            download_path_entry: self.download_path_entry.clone(),
            window: self.window.clone(),
        }
    }


    pub fn show(&self) {
        self.window.present();
    }
}

// Helper-Struktur für Event-Handler
#[allow(dead_code)]
#[derive(Clone)]
struct SettingsWindowClone {
    player_name_entry: Entry,
    download_path_entry: Entry,
    window: ApplicationWindow,
}

#[allow(dead_code)]
impl SettingsWindowClone {
    fn browse_for_path(&self) {
        let dialog = FileChooserDialog::builder()
            .title("Download-Pfad auswählen")
            .action(FileChooserAction::SelectFolder)
            .build();

        let entry_clone = self.download_path_entry.clone();
        let window_clone = self.window.clone();

        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        entry_clone.set_text(&path.to_string_lossy());
                    }
                }
            }
            dialog.close();
        });

        dialog.set_transient_for(Some(&window_clone));
        dialog.show();
    }

    fn save_and_close(&self) {
        let player_name = self.player_name_entry.text().to_string();
        let download_path_str = self.download_path_entry.text().to_string();
        let download_path = std::path::PathBuf::from(download_path_str);
        
        // Lade aktuelle Config, um peer_id zu behalten
        let current_config = Config::load();

        let config = Config {
            player_name,
            download_path,
            peer_id: current_config.peer_id,
            game_paths: current_config.game_paths, // Behalte vorhandene Spiel-Pfade
        };

        if let Err(e) = config.save() {
            eprintln!("Fehler beim Speichern der Konfiguration: {}", e);
        } else {
            self.window.close();
        }
    }
}

