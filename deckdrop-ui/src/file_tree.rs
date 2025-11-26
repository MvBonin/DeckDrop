use std::collections::HashMap;
use std::path::PathBuf;
use iced::{Element, Length, Color, Theme, Alignment};
use iced::widget::{column, row, text, Space, progress_bar, container};
use crate::app::{Message, scale, scale_text};

#[derive(Debug, Clone)]
pub struct FileTreeNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<FileTreeNode>,
    pub progress: f32, // 0.0 to 100.0
    pub size: u64,
    pub expanded: bool,
    pub is_complete: bool,
}

impl FileTreeNode {
    pub fn new_dir(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            is_dir: true,
            children: Vec::new(),
            progress: 0.0,
            size: 0,
            expanded: true, // Default expanded
            is_complete: false,
        }
    }

    pub fn new_file(name: String, path: PathBuf, size: u64, progress: f32, is_complete: bool) -> Self {
        Self {
            name,
            path,
            is_dir: false,
            children: Vec::new(),
            progress,
            size,
            expanded: false,
            is_complete,
        }
    }

    /// Baut einen Baum aus einer Liste von Pfaden und deren Fortschritt
    pub fn from_manifest(manifest: &deckdrop_core::synch::DownloadManifest) -> Self {
        let mut root = Self::new_dir("Game Root".to_string(), PathBuf::from(""));
        
        // Sortiere Dateien für konsistenten Baum
        let mut files: Vec<_> = manifest.chunks.iter().collect();
        files.sort_by(|(p1, _), (p2, _)| p1.cmp(p2));

        for (path_str, file_info) in files {
            let path = PathBuf::from(path_str);
            let size = file_info.file_size.unwrap_or(0);
            
            let is_complete = file_info.status == deckdrop_core::synch::DownloadStatus::Complete;
            
            // Überspringe fertige Dateien (werden nicht angezeigt)
            if is_complete {
                continue;
            }
            
            // Berechne Fortschritt pro Datei
            let downloaded = file_info.downloaded_chunks_count;
            let total = file_info.total_chunks;
            let progress = if total > 0 {
                (downloaded as f32 / total as f32) * 100.0
            } else {
                0.0 // Leere Datei hat keinen Fortschritt
            };

            root.insert_file(&path, size, progress, is_complete);
        }

        root.calculate_dir_stats();
        root
    }

    fn insert_file(&mut self, path: &PathBuf, size: u64, progress: f32, is_complete: bool) {
        let components: Vec<_> = path.components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        
        self.insert_recursive(&components, path, size, progress, is_complete);
    }

    fn insert_recursive(&mut self, components: &[String], full_path: &PathBuf, size: u64, progress: f32, is_complete: bool) {
        if components.is_empty() {
            return;
        }

        let name = &components[0];
        
        if components.len() == 1 {
            // Es ist die Datei selbst
            self.children.push(Self::new_file(name.clone(), full_path.clone(), size, progress, is_complete));
        } else {
            // Es ist ein Verzeichnis
            // Suche existierendes Verzeichnis oder erstelle neues
            let mut dir_idx = None;
            for (i, child) in self.children.iter().enumerate() {
                if child.is_dir && &child.name == name {
                    dir_idx = Some(i);
                    break;
                }
            }

            if dir_idx.is_none() {
                let new_dir = Self::new_dir(name.clone(), self.path.join(name));
                self.children.push(new_dir);
                dir_idx = Some(self.children.len() - 1);
            }

            if let Some(idx) = dir_idx {
                self.children[idx].insert_recursive(&components[1..], full_path, size, progress, is_complete);
            }
        }
    }

    /// Rekursives Berechnen von Ordner-Größe und Fortschritt (gewichtetes Mittel)
    fn calculate_dir_stats(&mut self) {
        if !self.is_dir {
            return;
        }

        let mut total_size = 0;
        let mut weighted_progress = 0.0;
        let mut all_children_complete = true;
        let mut has_children = false;

        for child in &mut self.children {
            has_children = true;
            child.calculate_dir_stats();
            total_size += child.size;
            weighted_progress += child.progress * (child.size as f32);
            if !child.is_complete {
                all_children_complete = false;
            }
        }

        self.size = total_size;
        if total_size > 0 {
            self.progress = weighted_progress / (total_size as f32);
        } else {
            self.progress = 100.0;
        }
        
        self.is_complete = has_children && all_children_complete;
    }

    /// Rendert den Baum (konsumiert self, um 'static Lifetime zu ermöglichen)
    pub fn view(self, indent: f32) -> Element<'static, Message> {
        let mut content = column![];

        // Zeile für dieses Element (Datei oder Ordner)
        let icon = if self.is_dir { "📁" } else { "📄" };
        
        let progress_text = format!("{:.1}%", self.progress);
        let size_text = format_size(self.size);

        // Wir müssen die Felder clonen/bewegen, da wir sie an text() übergeben
        let name = self.name.clone();
        let is_dir = self.is_dir;
        let expanded = self.expanded;
        let children = self.children; // Move children out
        let is_complete = self.is_complete;
        let progress = self.progress;

        let row_content = row![
            Space::with_width(Length::Fixed(indent)),
            text(icon).size(scale_text(14.0)),
            Space::with_width(Length::Fixed(scale(5.0))),
            text(name).size(scale_text(14.0)),
            Space::with_width(Length::Fill),
            text(size_text).size(scale_text(12.0)),
            Space::with_width(Length::Fixed(scale(10.0))),
            // Progress Bar or Done
            if is_complete {
                 container(
                    text("✓ Done").size(scale_text(12.0))
                        .style(|_theme: &Theme| {
                            iced::widget::text::Style {
                                color: Some(Color::from_rgba(0.0, 0.8, 0.0, 1.0)),
                            }
                        })
                 )
                 .width(Length::Fixed(scale(135.0))) // Gleiche Breite wie Bar + Text
                 .align_x(iced::Alignment::Center)
            } else {
                container(
                    row![
                        progress_bar(0.0..=100.0, progress)
                            .width(Length::Fixed(scale(100.0)))
                            .height(Length::Fixed(scale(8.0))),
                        Space::with_width(Length::Fixed(scale(5.0))),
                        text(progress_text).size(scale_text(12.0)),
                    ]
                    .align_y(iced::Alignment::Center)
                )
                .width(Length::Fixed(scale(135.0)))
            }
        ]
        .align_y(iced::Alignment::Center)
        .padding(scale(2.0));

        content = content.push(row_content);

        // Kinder rendern (wenn Ordner und ausgeklappt)
        if is_dir && expanded {
            for child in children {
                content = content.push(child.view(indent + scale(20.0)));
            }
        }

        content.into()
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
