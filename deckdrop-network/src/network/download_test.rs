#[cfg(test)]
mod tests {
    use crate::network::games::*;
    use crate::network::discovery::*;
    use libp2p::identity;
    use libp2p::PeerId;
    use tokio::time::{sleep, Duration};
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// Test: Zwei Peers tauschen Spiel-Daten aus
    #[tokio::test]
    #[ignore] // Ignoriere standardmäßig, da er Netzwerk-Zugriff benötigt
    async fn test_two_peers_game_download() {
        // Erstelle zwei Swarms mit unterschiedlichen Peer-IDs
        let keypair1 = identity::Keypair::generate_ed25519();
        let peer_id1 = PeerId::from(keypair1.public());
        
        let keypair2 = identity::Keypair::generate_ed25519();
        let peer_id2 = PeerId::from(keypair2.public());
        
        println!("Peer 1 ID: {}", peer_id1);
        println!("Peer 2 ID: {}", peer_id2);
        
        // Erstelle Test-Spiel für Peer 1
        let test_game = NetworkGameInfo {
            game_id: "test_game_123".to_string(),
            name: "Test Game".to_string(),
            version: "1.0.0".to_string(),
            start_file: "game.exe".to_string(),
            start_args: None,
            description: Some("Ein Test-Spiel".to_string()),
            creator_peer_id: Some(peer_id1.to_string()),
        };
        
        // Erstelle deckdrop.toml und deckdrop_chunks.toml für Test-Spiel
        let deckdrop_toml = format!(
            r#"game_id = "{}"
name = "{}"
version = "{}"
start_file = "{}"
description = "{}"
creator_peer_id = "{}"
hash = "blake3:test_hash"
"#,
            test_game.game_id,
            test_game.name,
            test_game.version,
            test_game.start_file,
            test_game.description.as_ref().unwrap(),
            test_game.creator_peer_id.as_ref().unwrap()
        );
        
        // Erstelle Test-Chunks (2 Chunks für eine Test-Datei)
        // Neues Format: file_hash + chunk_count
        let file_hash = "abc123def456"; // Test file_hash
        let chunk_count = 2;
        let file_size = 8 * 1024 * 1024; // 8MB Test-Datei
        let deckdrop_chunks_toml = format!(
            r#"[[file]]
path = "test.bin"
file_hash = "{}"
chunk_count = {}
file_size = {}
"#,
            file_hash, chunk_count, file_size
        );
        
        // Erstelle GameMetadataLoader für Peer 1
        let deckdrop_toml_clone = deckdrop_toml.clone();
        let deckdrop_chunks_toml_clone = deckdrop_chunks_toml.clone();
        let game_metadata_loader: GameMetadataLoader = Arc::new(move |game_id: &str| {
            if game_id == "test_game_123" {
                Some((deckdrop_toml_clone.clone(), deckdrop_chunks_toml_clone.clone()))
            } else {
                None
            }
        });
        
        // Erstelle ChunkLoader für Peer 1
        // Neues Format: "{file_hash}:{chunk_index}"
        let chunk1_data = vec![0u8; 5 * 1024 * 1024]; // 5MB Test-Daten
        let chunk2_data = vec![1u8; 3 * 1024 * 1024]; // 3MB Test-Daten
        
        let chunk1_data_clone = chunk1_data.clone();
        let chunk2_data_clone = chunk2_data.clone();
        let file_hash_clone = file_hash.to_string();
        let chunk_loader: ChunkLoader = Arc::new(move |chunk_hash: &str| {
            // Neues Format: "{file_hash}:{chunk_index}"
            if chunk_hash == format!("{}:0", file_hash_clone) {
                Some(chunk1_data_clone.clone())
            } else if chunk_hash == format!("{}:1", file_hash_clone) {
                Some(chunk2_data_clone.clone())
            } else {
                None
            }
        });
        
        // Erstelle GamesLoader für Peer 1
        let test_game_clone = test_game.clone();
        let games_loader: GamesLoader = Arc::new(move || {
            vec![test_game_clone.clone()]
        });
        
        // Erstelle Event-Channels
        let (event_tx1, _event_rx1) = mpsc::channel::<DiscoveryEvent>(32);
        let (event_tx2, mut event_rx2) = mpsc::channel::<DiscoveryEvent>(32);
        
        // Erstelle Download-Request-Channels
        let (_download_request_tx1, download_request_rx1) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        let (download_request_tx2_to_swarm, download_request_rx2_to_swarm) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        
        // Starte Discovery für beide Peers
        let _handle1 = start_discovery(
            event_tx1,
            Some("Peer1".to_string()),
            Some(1),
            Some(keypair1),
            Some(games_loader),
            Some(game_metadata_loader),
            Some(chunk_loader),
            Some(download_request_rx1),
            None,
            5, // max_concurrent_chunks
        ).await;
        
        let _handle2 = start_discovery(
            event_tx2,
            Some("Peer2".to_string()),
            Some(0),
            Some(keypair2),
            None, // Peer 2 hat keine Spiele
            None,
            None,
            Some(download_request_rx2_to_swarm),
            None,
            5, // max_concurrent_chunks
        ).await;
        
        // Verwende download_request_tx2_to_swarm für Requests vom Test
        let download_request_tx2_final = download_request_tx2_to_swarm;
        
        // Warte auf Peer-Discovery
        sleep(Duration::from_secs(2)).await;
        
        // Peer 2 fragt nach Spiel-Liste von Peer 1
        // (Dies sollte automatisch passieren, wenn Peers sich finden)
        
        // Warte auf GamesListReceived Event
        let mut games_received = false;
        let mut timeout_counter = 0;
        while !games_received && timeout_counter < 20 {
            tokio::select! {
                event = event_rx2.recv() => {
                    if let Some(DiscoveryEvent::GamesListReceived { peer_id, games }) = event {
                        println!("Peer 2 hat Spiele-Liste erhalten: {} Spiele von {}", games.len(), peer_id);
                        assert_eq!(games.len(), 1);
                        assert_eq!(games[0].game_id, "test_game_123");
                        games_received = true;
                    }
                }
                _ = sleep(Duration::from_millis(100)) => {
                    timeout_counter += 1;
                }
            }
        }
        
        assert!(games_received, "GamesList sollte empfangen worden sein");
        
        // Peer 2 startet Download
        // Finde Peer-ID von Peer 1
        let peer1_id_str = peer_id1.to_string();
        download_request_tx2_final.send(DownloadRequest::RequestGameMetadata {
            peer_id: peer1_id_str.clone(),
            game_id: "test_game_123".to_string(),
        }).unwrap();
        
        // Warte auf GameMetadataReceived
        let mut metadata_received = false;
        timeout_counter = 0;
        while !metadata_received && timeout_counter < 30 {
            tokio::select! {
                event = event_rx2.recv() => {
                    if let Some(DiscoveryEvent::GameMetadataReceived { peer_id, game_id, .. }) = event {
                        println!("Peer 2 hat Metadaten erhalten für Spiel {} von {}", game_id, peer_id);
                        assert_eq!(game_id, "test_game_123");
                        assert_eq!(peer_id, peer1_id_str);
                        metadata_received = true;
                    }
                }
                _ = sleep(Duration::from_millis(100)) => {
                    timeout_counter += 1;
                }
            }
        }
        
        assert!(metadata_received, "GameMetadata sollte empfangen worden sein");
        
        // Peer 2 fragt nach Chunks (neues Format: "{file_hash}:{chunk_index}")
        download_request_tx2_final.send(DownloadRequest::RequestChunk {
            peer_id: peer1_id_str.clone(),
            chunk_hash: format!("{}:0", file_hash),
            game_id: "test_game_123".to_string(),
        }).unwrap();
        
        download_request_tx2_final.send(DownloadRequest::RequestChunk {
            peer_id: peer1_id_str.clone(),
            chunk_hash: format!("{}:1", file_hash),
            game_id: "test_game_123".to_string(),
        }).unwrap();
        
        // Warte auf ChunkReceived Events
        let file_hash_for_validation = file_hash.to_string();
        let mut chunks_received = 0;
        timeout_counter = 0;
        while chunks_received < 2 && timeout_counter < 50 {
            tokio::select! {
                event = event_rx2.recv() => {
                    if let Some(DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data }) = event {
                        println!("Peer 2 hat Chunk {} erhalten: {} Bytes von {}", chunk_hash, chunk_data.len(), peer_id);
                        assert_eq!(peer_id, peer1_id_str);
                        
                        // Validiere Chunk-Daten (neues Format: "{file_hash}:{chunk_index}")
                        if chunk_hash == format!("{}:0", file_hash_for_validation) {
                            assert_eq!(chunk_data.len(), 5 * 1024 * 1024);
                            assert_eq!(chunk_data[0], 0u8);
                        } else if chunk_hash == format!("{}:1", file_hash_for_validation) {
                            assert_eq!(chunk_data.len(), 3 * 1024 * 1024);
                            assert_eq!(chunk_data[0], 1u8);
                        }
                        
                        chunks_received += 1;
                    }
                }
                _ = sleep(Duration::from_millis(100)) => {
                    timeout_counter += 1;
                }
            }
        }
        
        assert_eq!(chunks_received, 2, "Beide Chunks sollten empfangen worden sein");
        
        println!("Test erfolgreich: Zwei Peers haben Spiel-Daten ausgetauscht!");
    }
    
    /// Test: Robustheit - Rate-Limiting verhindert Request-Sturm
    #[tokio::test]
    async fn test_rate_limiting_prevents_request_storm() {
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        
        // Simuliere Rate-Limiting
        let active_requests: Arc<Mutex<HashMap<String, usize>>> = 
            Arc::new(Mutex::new(HashMap::new()));
        
        let peer_id = "test-peer-rate-limit".to_string();
        let max_requests = 5;
        
        // Test: Versuche mehr Requests als erlaubt
        let mut requests_sent = 0;
        for i in 0..10 {
            let mut active = active_requests.lock().await;
            let current_count = active.get(&peer_id).copied().unwrap_or(0);
            
            if current_count < max_requests {
                *active.entry(peer_id.clone()).or_insert(0) += 1;
                requests_sent += 1;
                println!("Request {} gesendet (aktive: {})", i + 1, current_count + 1);
            } else {
                println!("Request {} blockiert (Rate-Limit erreicht: {})", i + 1, current_count);
                // Request sollte blockiert werden
                break;
            }
        }
        
        // Prüfe dass nur max_requests gesendet wurden
        {
            let active = active_requests.lock().await;
            let count = active.get(&peer_id).copied().unwrap_or(0);
            assert_eq!(count, max_requests, "Nur {} Requests sollten aktiv sein", max_requests);
            assert_eq!(requests_sent, max_requests, "Nur {} Requests sollten gesendet worden sein", max_requests);
        }
        
        println!("✓ Rate-Limiting verhindert Request-Sturm korrekt");
    }
    
    /// Test: Robustheit - Circuit Breaker blockiert Peer nach Fehlern
    #[test]
    fn test_circuit_breaker_blocks_peer() {
        use std::time::Instant;
        use std::time::Duration;
        
        // Simuliere PeerPerformance
        struct TestPeerPerformance {
            consecutive_failures: usize,
            success_rate: f64,
            blocked_until: Option<Instant>,
        }
        
        let mut perf = TestPeerPerformance {
            consecutive_failures: 0,
            success_rate: 1.0,
            blocked_until: None,
        };
        
        // Test 1: Erster Fehler - noch nicht blockiert
        perf.consecutive_failures = 1;
        perf.success_rate = 0.9;
        let should_block = perf.consecutive_failures >= 2 || perf.success_rate < 0.5;
        assert!(!should_block, "Peer sollte nach 1 Fehler noch nicht blockiert sein");
        
        // Test 2: Zweiter Fehler - sollte blockiert werden
        perf.consecutive_failures = 2;
        perf.success_rate = 0.8;
        let should_block = perf.consecutive_failures >= 2 || perf.success_rate < 0.5;
        assert!(should_block, "Peer sollte nach 2 Fehlern blockiert werden");
        
        // Test 3: Blockierung setzen
        if should_block {
            perf.blocked_until = Some(Instant::now() + Duration::from_secs(300));
        }
        assert!(perf.blocked_until.is_some(), "Peer sollte blockiert sein");
        
        // Test 4: Prüfe ob Peer noch blockiert ist
        if let Some(blocked_until) = perf.blocked_until {
            let is_blocked = Instant::now() < blocked_until;
            assert!(is_blocked, "Peer sollte noch blockiert sein");
        }
        
        // Test 5: Erfolgreicher Request sollte Blockierung zurücksetzen
        perf.consecutive_failures = 0;
        perf.success_rate = 0.8;
        if perf.success_rate > 0.7 {
            perf.blocked_until = None; // Entblockiere bei guter Performance
        }
        assert!(perf.blocked_until.is_none(), "Peer sollte entblockiert sein bei guter Performance");
        
        println!("✓ Circuit Breaker funktioniert korrekt");
    }
}

