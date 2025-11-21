#[cfg(test)]
mod tests {
    use crate::network::games::*;
    use crate::network::discovery::*;
    use libp2p::identity;
    use libp2p::PeerId;
    use tokio::time::{sleep, Duration, timeout};
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use std::collections::HashMap;

    /// Umfassender Test: 2 größere Dateien zwischen 2 Peers übertragen
    /// Testet:
    /// - Parallele Downloads mehrerer Chunks
    /// - Robustheit bei Fehlern
    /// - Kontinuierlicher Download ohne Stopp
    #[tokio::test]
    #[ignore] // Ignoriere standardmäßig, da er Netzwerk-Zugriff benötigt
    async fn test_robust_download_two_large_files() {
        println!("=== Test: Robuster Download von 2 größeren Dateien ===");
        
        // Erstelle zwei Swarms mit unterschiedlichen Peer-IDs
        let keypair1 = identity::Keypair::generate_ed25519();
        let peer_id1 = PeerId::from(keypair1.public());
        
        let keypair2 = identity::Keypair::generate_ed25519();
        let peer_id2 = PeerId::from(keypair2.public());
        
        println!("Peer 1 ID: {}", peer_id1);
        println!("Peer 2 ID: {}", peer_id2);
        
        // Erstelle 2 Test-Spiele mit größeren Dateien
        // Spiel 1: 50MB Datei (5 Chunks à 10MB)
        // Spiel 2: 80MB Datei (8 Chunks à 10MB)
        
        let game1_id = "test_game_1_large";
        let game2_id = "test_game_2_large";
        
        let file1_hash = "file1_hash_abc123";
        let file2_hash = "file2_hash_def456";
        
        let file1_chunk_count = 5; // 5 Chunks à 10MB = 50MB
        let file2_chunk_count = 8; // 8 Chunks à 10MB = 80MB
        
        // Erstelle Test-Chunk-Daten
        let mut chunks_data: HashMap<String, Vec<u8>> = HashMap::new();
        
        // Datei 1: 5 Chunks
        for i in 0..file1_chunk_count {
            let chunk_hash = format!("{}:{}", file1_hash, i);
            let chunk_data = vec![i as u8; 10 * 1024 * 1024]; // 10MB pro Chunk
            chunks_data.insert(chunk_hash, chunk_data);
        }
        
        // Datei 2: 8 Chunks
        for i in 0..file2_chunk_count {
            let chunk_hash = format!("{}:{}", file2_hash, i);
            let chunk_data = vec![(i + 100) as u8; 10 * 1024 * 1024]; // 10MB pro Chunk
            chunks_data.insert(chunk_hash, chunk_data);
        }
        
        println!("Erstellt {} Chunks für Datei 1 ({} MB)", file1_chunk_count, file1_chunk_count * 10);
        println!("Erstellt {} Chunks für Datei 2 ({} MB)", file2_chunk_count, file2_chunk_count * 10);
        
        // Erstelle deckdrop.toml und deckdrop_chunks.toml für beide Spiele
        let deckdrop_toml1 = format!(
            r#"game_id = "{}"
name = "Test Game 1 (Large)"
version = "1.0.0"
start_file = "game1.exe"
description = "Test-Spiel mit größerer Datei"
creator_peer_id = "{}"
hash = "blake3:test_hash_1"
"#,
            game1_id, peer_id1
        );
        
        let deckdrop_chunks_toml1 = format!(
            r#"[[file]]
path = "game1.bin"
file_hash = "{}"
chunk_count = {}
file_size = {}
"#,
            file1_hash, file1_chunk_count, file1_chunk_count * 10 * 1024 * 1024
        );
        
        let deckdrop_toml2 = format!(
            r#"game_id = "{}"
name = "Test Game 2 (Large)"
version = "1.0.0"
start_file = "game2.exe"
description = "Test-Spiel mit noch größerer Datei"
creator_peer_id = "{}"
hash = "blake3:test_hash_2"
"#,
            game2_id, peer_id1
        );
        
        let deckdrop_chunks_toml2 = format!(
            r#"[[file]]
path = "game2.bin"
file_hash = "{}"
chunk_count = {}
file_size = {}
"#,
            file2_hash, file2_chunk_count, file2_chunk_count * 10 * 1024 * 1024
        );
        
        // Erstelle GameMetadataLoader für Peer 1
        let deckdrop_toml1_clone = deckdrop_toml1.clone();
        let deckdrop_chunks_toml1_clone = deckdrop_chunks_toml1.clone();
        let deckdrop_toml2_clone = deckdrop_toml2.clone();
        let deckdrop_chunks_toml2_clone = deckdrop_chunks_toml2.clone();
        
        let game_metadata_loader: GameMetadataLoader = Arc::new(move |game_id: &str| {
            if game_id == game1_id {
                Some((deckdrop_toml1_clone.clone(), deckdrop_chunks_toml1_clone.clone()))
            } else if game_id == game2_id {
                Some((deckdrop_toml2_clone.clone(), deckdrop_chunks_toml2_clone.clone()))
            } else {
                None
            }
        });
        
        // Erstelle ChunkLoader für Peer 1
        let chunks_data_clone = chunks_data.clone();
        let chunk_loader: ChunkLoader = Arc::new(move |chunk_hash: &str| {
            chunks_data_clone.get(chunk_hash).cloned()
        });
        
        // Erstelle GamesLoader für Peer 1
        let test_game1 = NetworkGameInfo {
            game_id: game1_id.to_string(),
            name: "Test Game 1 (Large)".to_string(),
            version: "1.0.0".to_string(),
            start_file: "game1.exe".to_string(),
            start_args: None,
            description: Some("Test-Spiel mit größerer Datei".to_string()),
            creator_peer_id: Some(peer_id1.to_string()),
        };
        
        let test_game2 = NetworkGameInfo {
            game_id: game2_id.to_string(),
            name: "Test Game 2 (Large)".to_string(),
            version: "1.0.0".to_string(),
            start_file: "game2.exe".to_string(),
            start_args: None,
            description: Some("Test-Spiel mit noch größerer Datei".to_string()),
            creator_peer_id: Some(peer_id1.to_string()),
        };
        
        let games_loader: GamesLoader = Arc::new(move || {
            vec![test_game1.clone(), test_game2.clone()]
        });
        
        // Erstelle Event-Channels
        let (event_tx1, _event_rx1) = mpsc::channel::<DiscoveryEvent>(128);
        let (event_tx2, mut event_rx2) = mpsc::channel::<DiscoveryEvent>(128);
        
        // Erstelle Download-Request-Channels
        let (_download_request_tx1, download_request_rx1) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        let (download_request_tx2_to_swarm, download_request_rx2_to_swarm) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        
        // Starte Discovery für beide Peers mit höherem max_concurrent_chunks für bessere Parallelisierung
        let _handle1 = start_discovery(
            event_tx1,
            Some("Peer1".to_string()),
            Some(2), // 2 Spiele
            Some(keypair1),
            Some(games_loader),
            Some(game_metadata_loader),
            Some(chunk_loader),
            Some(download_request_rx1),
            None,
            15, // max_concurrent_chunks: 15 für bessere Parallelisierung
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
            15, // max_concurrent_chunks: 15 für bessere Parallelisierung
        ).await;
        
        let download_request_tx2_final = download_request_tx2_to_swarm;
        let peer1_id_str = peer_id1.to_string();
        
        // Warte auf Peer-Discovery
        println!("Warte auf Peer-Discovery...");
        sleep(Duration::from_secs(3)).await;
        
        // Peer 2 fragt nach Spiel-Liste von Peer 1
        println!("Peer 2 wartet auf GamesList...");
        let mut games_received = false;
        let mut actual_game1_id: Option<String> = None;
        let mut actual_game2_id: Option<String> = None;
        
        let peer1_id_str_clone = peer1_id_str.clone();
        let games_received_result = timeout(Duration::from_secs(10), async {
            while let Some(event) = event_rx2.recv().await {
                if let DiscoveryEvent::GamesListReceived { peer_id, games } = event {
                    // Nur Spiele von Peer 1 akzeptieren (ignoriere andere Peers im Netzwerk)
                    if peer_id != peer1_id_str_clone {
                        println!("Ignoriere GamesList von anderem Peer: {} (erwartet: {})", peer_id, peer1_id_str_clone);
                        continue;
                    }
                    
                    println!("✓ Peer 2 hat Spiele-Liste erhalten: {} Spiele von {}", games.len(), peer_id);
                    assert_eq!(games.len(), 2, "Es sollten 2 Spiele sein");
                    
                    // Debug: Zeige alle game_ids
                    for (i, game) in games.iter().enumerate() {
                        println!("  Spiel {}: game_id={}, name={}", i, game.game_id, game.name);
                    }
                    
                    // Prüfe ob die Spiele die erwarteten Namen haben (game_id könnte gehasht sein)
                    let game1_found = games.iter().any(|g| g.name == "Test Game 1 (Large)");
                    let game2_found = games.iter().any(|g| g.name == "Test Game 2 (Large)");
                    assert!(game1_found, "Spiel 1 sollte gefunden werden");
                    assert!(game2_found, "Spiel 2 sollte gefunden werden");
                    
                    // Speichere die tatsächlichen game_ids für später
                    actual_game1_id = games.iter().find(|g| g.name == "Test Game 1 (Large)").map(|g| g.game_id.clone());
                    actual_game2_id = games.iter().find(|g| g.name == "Test Game 2 (Large)").map(|g| g.game_id.clone());
                    
                    if let (Some(ref id1), Some(ref id2)) = (&actual_game1_id, &actual_game2_id) {
                        println!("  Tatsächliche game_ids: Spiel 1={}, Spiel 2={}", id1, id2);
                    }
                    
                    games_received = true;
                    break;
                }
            }
            (actual_game1_id.clone(), actual_game2_id.clone())
        }).await;
        
        assert!(games_received_result.is_ok() && games_received, "GamesList sollte empfangen worden sein");
        
        // Extrahiere die tatsächlichen game_ids
        let (actual_id1, actual_id2) = games_received_result.unwrap();
        let actual_game1_id_final = actual_id1.expect("Spiel 1 game_id sollte gefunden worden sein");
        let actual_game2_id_final = actual_id2.expect("Spiel 2 game_id sollte gefunden worden sein");
        
        println!("Verwende game_ids: Spiel 1={}, Spiel 2={}", actual_game1_id_final, actual_game2_id_final);
        
        // Test 1: Download von Spiel 1 (5 Chunks)
        println!("\n=== Test 1: Download von Spiel 1 ({} Chunks) ===", file1_chunk_count);
        
        // Peer 2 startet Download von Spiel 1 (verwende die tatsächliche game_id)
        download_request_tx2_final.send(DownloadRequest::RequestGameMetadata {
            peer_id: peer1_id_str.clone(),
            game_id: actual_game1_id_final.clone(),
        }).unwrap();
        
        // Warte auf GameMetadataReceived
        let mut metadata1_received = false;
        let metadata1_result = timeout(Duration::from_secs(10), async {
            while let Some(event) = event_rx2.recv().await {
                if let DiscoveryEvent::GameMetadataReceived { peer_id, game_id, .. } = event {
                    if game_id == actual_game1_id_final {
                        println!("✓ Peer 2 hat Metadaten für Spiel 1 erhalten von {}", peer_id);
                        assert_eq!(peer_id, peer1_id_str);
                        metadata1_received = true;
                        break;
                    }
                }
            }
        }).await;
        
        assert!(metadata1_result.is_ok() && metadata1_received, "GameMetadata für Spiel 1 sollte empfangen worden sein");
        
        // Fordere alle Chunks von Spiel 1 an (sollte parallel passieren)
        println!("Fordere alle {} Chunks von Spiel 1 an...", file1_chunk_count);
        for i in 0..file1_chunk_count {
            let chunk_hash = format!("{}:{}", file1_hash, i);
            download_request_tx2_final.send(DownloadRequest::RequestChunk {
                peer_id: peer1_id_str.clone(),
                chunk_hash: chunk_hash.clone(),
                game_id: actual_game1_id_final.clone(),
            }).unwrap();
        }
        
        // Warte auf alle Chunks von Spiel 1
        let mut chunks1_received: HashMap<String, Vec<u8>> = HashMap::new();
        let mut last_progress_time = std::time::Instant::now();
        let chunks1_result = timeout(Duration::from_secs(60), async {
            while chunks1_received.len() < file1_chunk_count {
                // Timeout nach 10 Sekunden ohne Fortschritt
                if last_progress_time.elapsed() > Duration::from_secs(10) {
                    eprintln!("⚠️ Kein Fortschritt seit 10 Sekunden. Empfangen: {}/{} Chunks", 
                        chunks1_received.len(), file1_chunk_count);
                    eprintln!("  Fehlende Chunks: {:?}", 
                        (0..file1_chunk_count)
                            .map(|i| format!("{}:{}", file1_hash, i))
                            .filter(|hash| !chunks1_received.contains_key(hash))
                            .collect::<Vec<_>>());
                    last_progress_time = std::time::Instant::now();
                }
                
                // Timeout für Event-Empfang
                match tokio::time::timeout(Duration::from_secs(5), event_rx2.recv()).await {
                    Ok(Some(event)) => {
                        if let DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data } = event {
                            if chunk_hash.starts_with(&format!("{}:", file1_hash)) {
                                println!("✓ Chunk {} erhalten: {} MB von {}", 
                                    chunk_hash, chunk_data.len() / (1024 * 1024), peer_id);
                                assert_eq!(peer_id, peer1_id_str);
                                assert_eq!(chunk_data.len(), 10 * 1024 * 1024, "Chunk sollte 10MB sein");
                                
                                // Validiere Chunk-Daten
                                let chunk_index: usize = chunk_hash.split(':').nth(1).unwrap().parse().unwrap();
                                let expected_byte = chunk_index as u8;
                                assert_eq!(chunk_data[0], expected_byte, "Chunk-Daten sollten korrekt sein");
                                
                                chunks1_received.insert(chunk_hash, chunk_data);
                                println!("  Fortschritt: {}/{} Chunks", chunks1_received.len(), file1_chunk_count);
                                last_progress_time = std::time::Instant::now();
                            }
                        } else if let DiscoveryEvent::ChunkRequestFailed { chunk_hash, error, .. } = event {
                            if chunk_hash.starts_with(&format!("{}:", file1_hash)) {
                                eprintln!("❌ Chunk {} fehlgeschlagen: {}", chunk_hash, error);
                                // Retry sollte automatisch passieren
                            }
                        } else if let DiscoveryEvent::ChunkRequestSent { peer_id, chunk_hash, game_id } = event {
                            if chunk_hash.starts_with(&format!("{}:", file1_hash)) {
                                println!("✓ ChunkRequestSent Event empfangen: {} von {} für {}", chunk_hash, peer_id, game_id);
                            }
                        }
                    }
                    Ok(None) => {
                        eprintln!("⚠️ Event-Kanal geschlossen");
                        break;
                    }
                    Err(_) => {
                        eprintln!("⚠️ Timeout beim Warten auf Events (5s)");
                        // Weiter versuchen
                    }
                }
            }
        }).await;
        
        assert!(chunks1_result.is_ok(), "Alle Chunks von Spiel 1 sollten innerhalb von 60s empfangen worden sein");
        assert_eq!(chunks1_received.len(), file1_chunk_count, "Alle {} Chunks von Spiel 1 sollten empfangen worden sein", file1_chunk_count);
        println!("✓ Spiel 1 komplett heruntergeladen: {} Chunks", chunks1_received.len());
        
        // Test 2: Download von Spiel 2 (8 Chunks) - während Spiel 1 noch läuft
        println!("\n=== Test 2: Download von Spiel 2 ({} Chunks) ===", file2_chunk_count);
        
        // Peer 2 startet Download von Spiel 2 (verwende die tatsächliche game_id)
        download_request_tx2_final.send(DownloadRequest::RequestGameMetadata {
            peer_id: peer1_id_str.clone(),
            game_id: actual_game2_id_final.clone(),
        }).unwrap();
        
        // Warte auf GameMetadataReceived
        let mut metadata2_received = false;
        let metadata2_result = timeout(Duration::from_secs(10), async {
            while let Some(event) = event_rx2.recv().await {
                if let DiscoveryEvent::GameMetadataReceived { peer_id, game_id, .. } = event {
                    if game_id == actual_game2_id_final {
                        println!("✓ Peer 2 hat Metadaten für Spiel 2 erhalten von {}", peer_id);
                        assert_eq!(peer_id, peer1_id_str);
                        metadata2_received = true;
                        break;
                    }
                }
            }
        }).await;
        
        assert!(metadata2_result.is_ok() && metadata2_received, "GameMetadata für Spiel 2 sollte empfangen worden sein");
        
        // Fordere alle Chunks von Spiel 2 an
        println!("Fordere alle {} Chunks von Spiel 2 an...", file2_chunk_count);
        for i in 0..file2_chunk_count {
            let chunk_hash = format!("{}:{}", file2_hash, i);
            download_request_tx2_final.send(DownloadRequest::RequestChunk {
                peer_id: peer1_id_str.clone(),
                chunk_hash: chunk_hash.clone(),
                game_id: actual_game2_id_final.clone(),
            }).unwrap();
        }
        
        // Warte auf alle Chunks von Spiel 2
        let mut chunks2_received: HashMap<String, Vec<u8>> = HashMap::new();
        let mut last_progress_time2 = std::time::Instant::now();
        let chunks2_result = timeout(Duration::from_secs(90), async {
            while chunks2_received.len() < file2_chunk_count {
                // Timeout nach 10 Sekunden ohne Fortschritt
                if last_progress_time2.elapsed() > Duration::from_secs(10) {
                    eprintln!("⚠️ Kein Fortschritt seit 10 Sekunden. Empfangen: {}/{} Chunks", 
                        chunks2_received.len(), file2_chunk_count);
                    eprintln!("  Fehlende Chunks: {:?}", 
                        (0..file2_chunk_count)
                            .map(|i| format!("{}:{}", file2_hash, i))
                            .filter(|hash| !chunks2_received.contains_key(hash))
                            .collect::<Vec<_>>());
                    last_progress_time2 = std::time::Instant::now();
                }
                
                // Timeout für Event-Empfang
                match tokio::time::timeout(Duration::from_secs(5), event_rx2.recv()).await {
                    Ok(Some(event)) => {
                        if let DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data } = event {
                            if chunk_hash.starts_with(&format!("{}:", file2_hash)) {
                                println!("✓ Chunk {} erhalten: {} MB von {}", 
                                    chunk_hash, chunk_data.len() / (1024 * 1024), peer_id);
                                assert_eq!(peer_id, peer1_id_str);
                                assert_eq!(chunk_data.len(), 10 * 1024 * 1024, "Chunk sollte 10MB sein");
                                
                                // Validiere Chunk-Daten
                                let chunk_index: usize = chunk_hash.split(':').nth(1).unwrap().parse().unwrap();
                                let expected_byte = (chunk_index + 100) as u8;
                                assert_eq!(chunk_data[0], expected_byte, "Chunk-Daten sollten korrekt sein");
                                
                                chunks2_received.insert(chunk_hash, chunk_data);
                                println!("  Fortschritt: {}/{} Chunks", chunks2_received.len(), file2_chunk_count);
                                last_progress_time2 = std::time::Instant::now();
                            }
                        } else if let DiscoveryEvent::ChunkRequestFailed { chunk_hash, error, .. } = event {
                            if chunk_hash.starts_with(&format!("{}:", file2_hash)) {
                                eprintln!("❌ Chunk {} fehlgeschlagen: {}", chunk_hash, error);
                                // Retry sollte automatisch passieren
                            }
                        } else if let DiscoveryEvent::ChunkRequestSent { .. } = event {
                            // Ignoriere ChunkRequestSent Events
                        }
                    }
                    Ok(None) => {
                        eprintln!("⚠️ Event-Kanal geschlossen");
                        break;
                    }
                    Err(_) => {
                        eprintln!("⚠️ Timeout beim Warten auf Events (5s)");
                        // Weiter versuchen
                    }
                }
            }
        }).await;
        
        assert!(chunks2_result.is_ok(), "Alle Chunks von Spiel 2 sollten innerhalb von 90s empfangen worden sein");
        assert_eq!(chunks2_received.len(), file2_chunk_count, "Alle {} Chunks von Spiel 2 sollten empfangen worden sein", file2_chunk_count);
        println!("✓ Spiel 2 komplett heruntergeladen: {} Chunks", chunks2_received.len());
        
        println!("\n=== Test erfolgreich abgeschlossen ===");
        println!("✓ Spiel 1: {} Chunks ({} MB)", chunks1_received.len(), chunks1_received.len() * 10);
        println!("✓ Spiel 2: {} Chunks ({} MB)", chunks2_received.len(), chunks2_received.len() * 10);
        println!("✓ Gesamt: {} Chunks ({} MB)", 
            chunks1_received.len() + chunks2_received.len(),
            (chunks1_received.len() + chunks2_received.len()) * 10);
    }
}

