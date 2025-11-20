//! deckdrop-core - Core-Logik ohne UI-Abhängigkeiten
//!
//! Dieses Paket enthält alle Core-Funktionalität:
//! - Konfigurationsverwaltung
//! - Spiel-Verwaltung
//! - Download-Synchronisation
//! - Game-Integritätsprüfung

pub mod config;
pub mod game;
pub mod gamechecker;
pub mod network_cache;
pub mod synch;

// Re-export wichtige Typen für einfacheren Zugriff
pub use config::Config;
pub use game::{GameInfo, load_games_from_directory, check_game_config_exists, check_complete_deckdrop_game_exists, generate_chunks_toml};
pub use gamechecker::{calculate_file_hash, verify_game_integrity, verify_game_integrity_with_progress, light_check_game};
pub use synch::{
    DownloadManifest, DownloadStatus, DownloadProgress,
    start_game_download, request_missing_chunks, 
    finalize_game_download, cancel_game_download,
    get_manifest_path, get_chunks_dir, find_game_id_for_chunk,
    check_and_reconstruct_files, check_and_reconstruct_single_file, find_file_for_chunk, load_active_downloads,
    save_chunk, validate_chunk_size,
};

