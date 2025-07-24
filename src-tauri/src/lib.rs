// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::Manager;
use network::{channel::new_peer_channel, discovery::run_discovery, peer::PeerInfo};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

pub mod network;
pub mod config;

use network::{discovery, peer, channel};
use config::{get_config, update_player_name, update_games_folder, update_theme, save_initial_config, check_config_exists};

// Store discovered peers
lazy_static::lazy_static! {
    static ref PEERS: Arc<Mutex<HashMap<String, peer::PeerInfo>>> = Arc::new(Mutex::new(HashMap::new()));
}

#[tauri::command]
async fn greet(name: &str) -> Result<String, String> {
    Ok(format!("Hello, {}! You've been greeted from Rust!", name))
}

#[tauri::command]
async fn get_discovered_peers() -> Result<Vec<peer::PeerInfo>, String> {
    let peers = PEERS.lock().await;
    Ok(peers.values().cloned().collect())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            greet,
            get_discovered_peers,
            get_config,
            update_player_name,
            update_games_folder,
            update_theme,
            save_initial_config,
            check_config_exists
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greet_function() {
        let result = greet("World");
        assert_eq!(result, "Hello, World! You've been greeted from Rust!");
        
        let result = greet("Alice");
        assert_eq!(result, "Hello, Alice! You've been greeted from Rust!");
    }

    #[test]
    fn test_greet_empty_string() {
        let result = greet("");
        assert_eq!(result, "Hello, ! You've been greeted from Rust!");
    }

    #[test]
    fn test_greet_special_characters() {
        let result = greet("Test@123!");
        assert_eq!(result, "Hello, Test@123!! You've been greeted from Rust!");
    }
}
