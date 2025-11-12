use crate::config::Config;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Button, Entry, FileChooserDialog, FileChooserAction, 
           ResponseType, Label, Box as GtkBox, Orientation, Align};

pub struct SettingsWindow {
    window: ApplicationWindow,
    player_name_entry: Entry,
    games_path_entry: Entry,
    config: Config,
}

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

        // Spiele-Pfad
        let path_label = Label::new(Some("Spiele-Pfad:"));
        path_label.set_halign(Align::Start);
        main_box.append(&path_label);

        let path_box = GtkBox::new(Orientation::Horizontal, 8);
        
        let games_path_entry = Entry::new();
        games_path_entry.set_text(&config.games_path.to_string_lossy());
        games_path_entry.set_hexpand(true);
        path_box.append(&games_path_entry);

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
            games_path_entry,
            config: config.clone(),
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
            games_path_entry: self.games_path_entry.clone(),
            window: self.window.clone(),
        }
    }

    fn browse_for_path(&self) {
        // Diese Methode wird nicht direkt verwendet, sondern über clone_for_events
    }

    fn save_and_close(&self) {
        // Diese Methode wird nicht direkt verwendet, sondern über clone_for_events
    }

    pub fn show(&self) {
        self.window.present();
    }
}

// Helper-Struktur für Event-Handler
#[derive(Clone)]
struct SettingsWindowClone {
    player_name_entry: Entry,
    games_path_entry: Entry,
    window: ApplicationWindow,
}

impl SettingsWindowClone {
    fn browse_for_path(&self) {
        let dialog = FileChooserDialog::builder()
            .title("Spiele-Pfad auswählen")
            .action(FileChooserAction::SelectFolder)
            .build();

        let entry_clone = self.games_path_entry.clone();
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

        dialog.set_transient_for(&window_clone);
        dialog.show();
    }

    fn save_and_close(&self) {
        let player_name = self.player_name_entry.text().to_string();
        let games_path_str = self.games_path_entry.text().to_string();
        let games_path = std::path::PathBuf::from(games_path_str);

        let config = Config {
            player_name,
            games_path,
        };

        if let Err(e) = config.save() {
            eprintln!("Fehler beim Speichern der Konfiguration: {}", e);
        } else {
            self.window.close();
        }
    }
}

