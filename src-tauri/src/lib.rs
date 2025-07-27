// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use network::{channel::new_peer_channel, discovery::run_discovery};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

pub mod network;
pub mod config;

use network::peer;
use config::{get_config, update_player_name, update_games_folder, update_theme, save_initial_config, check_config_exists};

// Store discovered peers
lazy_static::lazy_static! {
    static ref PEERS: Arc<Mutex<HashMap<String, peer::PeerInfo>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref OUR_PEER_ID: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
}

#[tauri::command]
async fn greet(name: &str) -> Result<String, String> {
    Ok(format!("Hello, {}! You've been greeted from Rust!", name))
}

#[tauri::command]
async fn get_discovered_peers() -> Result<Vec<peer::PeerInfo>, String> {
    // Check if our peer ID is set
    let our_id = OUR_PEER_ID.lock().await;
    if our_id.is_none() {
        println!("Warning: Our peer ID is not set yet");
        eprintln!("Warning: Our peer ID is not set yet");
    }
    
    let peers = PEERS.lock().await;
    let mut peer_list: Vec<peer::PeerInfo> = peers.values().cloned().collect();
    
    // Try to get our own player name to include in peer info
    let our_player_name = match get_config().await {
        Ok(config) => Some(config.player_name),
        Err(_) => None,
    };
    
    // Add our player name to peers that don't have one
    for peer in &mut peer_list {
        if peer.player_name.is_none() {
            peer.player_name = our_player_name.clone();
        }
    }
    
    println!("Returning {} discovered peers", peer_list.len());
    eprintln!("Returning {} discovered peers", peer_list.len());
    
    // Debug: Print all peer IDs
    for (i, peer) in peer_list.iter().enumerate() {
        println!("Peer {}: {} at {:?} (player: {:?})", i, peer.id, peer.addr, peer.player_name);
        eprintln!("Peer {}: {} at {:?} (player: {:?})", i, peer.id, peer.addr, peer.player_name);
    }
    
    Ok(peer_list)
}

#[tauri::command]
async fn get_mdns_status() -> Result<String, String> {
    let our_id = OUR_PEER_ID.lock().await;
    let peers = PEERS.lock().await;
    
    let status = format!(
        "Our Peer ID: {:?}\nTotal Peers: {}\nPeer IDs: {:?}",
        our_id.as_ref().map(|id| id.as_str()),
        peers.len(),
        peers.keys().collect::<Vec<_>>()
    );
    
    Ok(status)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Start mDNS discovery in background using std::thread
            let (sender, mut receiver) = new_peer_channel();
            
            // Generate our own peer ID
            let our_peer_id = {
                let id_keys = libp2p::identity::Keypair::generate_ed25519();
                let peer_id = libp2p::PeerId::from(id_keys.public());
                let peer_id_string = peer_id.to_string();
                
                // Store our peer ID
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let mut our_id = OUR_PEER_ID.lock().await;
                    *our_id = Some(peer_id_string.clone());
                    println!("Stored our peer ID: {}", peer_id_string);
                    eprintln!("Stored our peer ID: {}", peer_id_string);
                });
                
                peer_id_string
            };
            
            // Spawn discovery task in a separate thread
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    // Small delay to ensure our peer ID is stored
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    run_discovery(sender, Some(our_peer_id)).await;
                });
            });
            
            // Spawn peer update handler in a separate thread
            let _app_handle = app.handle();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    while let Ok(peer_info) = receiver.recv().await {
                        println!("Received peer update: {:?}", peer_info);
                        eprintln!("Received peer update: {:?}", peer_info);
                        let mut peers = PEERS.lock().await;
                        
                        // Get our own peer ID
                        let our_id = OUR_PEER_ID.lock().await;
                        let our_peer_id = our_id.as_ref();
                        
                        println!("Processing peer: {} (our ID: {:?})", peer_info.id, our_peer_id);
                        eprintln!("Processing peer: {} (our ID: {:?})", peer_info.id, our_peer_id);
                        
                        // Only add peers that are different from our own
                        if let Some(our_id) = our_peer_id {
                            if peer_info.id != *our_id {
                                // Check if this peer already exists with a different address
                                if let Some(existing_peer) = peers.get(&peer_info.id) {
                                    // Update the address if it's different
                                    if existing_peer.addr != peer_info.addr {
                                        peers.insert(peer_info.id.clone(), peer_info);
                                        println!("Updated peer address. Total peers in store: {}", peers.len());
                                        eprintln!("Updated peer address. Total peers in store: {}", peers.len());
                                    } else {
                                        println!("Peer already exists with same address: {}", peer_info.id);
                                        eprintln!("Peer already exists with same address: {}", peer_info.id);
                                    }
                                } else {
                                    peers.insert(peer_info.id.clone(), peer_info);
                                    println!("Added new peer. Total peers in store: {}", peers.len());
                                    eprintln!("Added new peer. Total peers in store: {}", peers.len());
                                }
                            } else {
                                println!("Ignoring our own peer: {}", peer_info.id);
                                eprintln!("Ignoring our own peer: {}", peer_info.id);
                            }
                            
                            // Clean up any peers that might be our own (from before we knew our ID)
                            peers.retain(|peer_id, _| peer_id != our_id);
                        } else {
                            // If we don't know our own ID yet, temporarily add the peer
                            // We'll clean this up later when we know our own ID
                            peers.insert(peer_info.id.clone(), peer_info);
                            println!("Added peer (our ID unknown). Total peers in store: {}", peers.len());
                            eprintln!("Added peer (our ID unknown). Total peers in store: {}", peers.len());
                        }
                    }
                });
            });
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_discovered_peers,
            get_mdns_status,
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

    #[tokio::test]
    async fn test_greet_function() {
        let result = greet("World").await.unwrap();
        assert_eq!(result, "Hello, World! You've been greeted from Rust!");
        
        let result = greet("Alice").await.unwrap();
        assert_eq!(result, "Hello, Alice! You've been greeted from Rust!");
    }

    #[tokio::test]
    async fn test_greet_empty_string() {
        let result = greet("").await.unwrap();
        assert_eq!(result, "Hello, ! You've been greeted from Rust!");
    }

    #[tokio::test]
    async fn test_greet_special_characters() {
        let result = greet("Test@123!").await.unwrap();
        assert_eq!(result, "Hello, Test@123!! You've been greeted from Rust!");
    }
}
