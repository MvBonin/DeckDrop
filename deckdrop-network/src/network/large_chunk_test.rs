#[cfg(test)]
mod tests {
    use tokio::time::{sleep, Duration, timeout};
    use std::collections::HashMap;
    use std::sync::Arc;
    use libp2p::identity;
    use tokio::sync::mpsc;
    use crate::network::discovery::{start_discovery, DiscoveryEvent, DownloadRequest};
    use crate::network::{ChunkLoader};

    /// Test mit 10MB Chunk um zu prüfen, ob das Problem die Größe ist
    #[tokio::test]
    #[ignore]
    async fn test_large_chunk_transfer_10mb() {
        println!("\n=== TEST: Großer Chunk-Transfer (10MB) ===");
        
        // Erstelle zwei Peers
        let keypair1 = identity::Keypair::generate_ed25519();
        let peer_id1 = keypair1.public().to_peer_id();
        let peer_id1_str = peer_id1.to_string();
        
        let keypair2 = identity::Keypair::generate_ed25519();
        let peer_id2 = keypair2.public().to_peer_id();
        let peer_id2_str = peer_id2.to_string();
        
        println!("Peer 1 (Uploader) ID: {}", peer_id1_str);
        println!("Peer 2 (Downloader) ID: {}", peer_id2_str);
        
        // Erstelle Test-Chunk-Daten (10MB)
        let chunk_hash = "test_chunk:0";
        let chunk_size = 10 * 1024 * 1024; // 10MB
        let chunk_data: Vec<u8> = (0..chunk_size).map(|i| (i % 256) as u8).collect();
        
        let mut chunks_data: HashMap<String, Vec<u8>> = HashMap::new();
        chunks_data.insert(chunk_hash.to_string(), chunk_data.clone());
        
        // Peer 1: Hat den Chunk (Uploader)
        let chunk_loader1: ChunkLoader = Arc::new(move |hash: &str| {
            println!("[Peer 1] ChunkLoader aufgerufen für: {} ({} MB)", hash, 
                chunks_data.get(hash).map(|d| d.len() / (1024 * 1024)).unwrap_or(0));
            chunks_data.get(hash).cloned()
        });
        
        // Peer 2: Will den Chunk (Downloader)
        let chunk_loader2: ChunkLoader = Arc::new(move |_hash: &str| {
            println!("[Peer 2] ChunkLoader aufgerufen (sollte nicht passieren)");
            None
        });
        
        // Event-Channels
        let (event_tx1, mut event_rx1) = mpsc::channel::<DiscoveryEvent>(128);
        let (event_tx2, mut event_rx2) = mpsc::channel::<DiscoveryEvent>(128);
        
        // Download-Request-Channels
        let (_download_request_tx1, download_request_rx1) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        let (download_request_tx2, download_request_rx2) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        
        // Starte Discovery
        println!("Starte Peer 1 (Uploader)...");
        let _handle1 = start_discovery(
            event_tx1,
            Some("Peer1".to_string()),
            Some(0),
            Some(keypair1),
            None,
            None,
            Some(chunk_loader1),
            Some(download_request_rx1),
            None,
            15,
        ).await;
        
        println!("Starte Peer 2 (Downloader)...");
        let _handle2 = start_discovery(
            event_tx2,
            Some("Peer2".to_string()),
            Some(0),
            Some(keypair2),
            None,
            None,
            Some(chunk_loader2),
            Some(download_request_rx2),
            None,
            15,
        ).await;
        
        // Warte auf Peer-Discovery
        println!("Warte 5 Sekunden auf Peer-Discovery...");
        sleep(Duration::from_secs(5)).await;
        
        // Peer 2 sendet Chunk-Request
        println!("\n[Peer 2] Sende Chunk-Request für: {} (10MB)", chunk_hash);
        download_request_tx2.send(DownloadRequest::RequestChunk {
            peer_id: peer_id1_str.clone(),
            chunk_hash: chunk_hash.to_string(),
            game_id: "test_game".to_string(),
        }).unwrap();
        
        // Warte auf Events
        println!("[Peer 2] Warte auf Chunk-Response (10MB)...");
        let result = timeout(Duration::from_secs(120), async {
            let mut chunk_received = false;
            let mut request_sent_received = false;
            
            loop {
                tokio::select! {
                    event = event_rx2.recv() => {
                        if let Some(event) = event {
                            match event {
                                DiscoveryEvent::ChunkRequestSent { peer_id, chunk_hash: hash, .. } => {
                                    println!("[Peer 2] ✓ ChunkRequestSent: {} von {}", hash, peer_id);
                                    request_sent_received = true;
                                }
                                DiscoveryEvent::ChunkReceived { peer_id, chunk_hash: hash, chunk_data: data } => {
                                    println!("[Peer 2] ✓✓✓ ChunkReceived: {} von {} ({} MB)", 
                                        hash, peer_id, data.len() / (1024 * 1024));
                                    
                                    // Validiere Daten
                                    assert_eq!(hash, chunk_hash);
                                    assert_eq!(data.len(), chunk_size);
                                    
                                    // Validiere Dateninhalt (nur erste und letzte Bytes für Performance)
                                    if data.len() > 0 {
                                        let first_expected = 0u8;
                                        let last_expected = ((data.len() - 1) % 256) as u8;
                                        if data[0] != first_expected {
                                            panic!("Datenfehler an Position 0: erwartet {}, erhalten {}", first_expected, data[0]);
                                        }
                                        if data[data.len() - 1] != last_expected {
                                            panic!("Datenfehler an Position {}: erwartet {}, erhalten {}", data.len() - 1, last_expected, data[data.len() - 1]);
                                        }
                                    }
                                    
                                    println!("[Peer 2] ✓✓✓ Datenvalidierung erfolgreich!");
                                    chunk_received = true;
                                    break;
                                }
                                DiscoveryEvent::ChunkRequestFailed { chunk_hash: hash, error, .. } => {
                                    eprintln!("[Peer 2] ❌ ChunkRequestFailed: {} - {}", hash, error);
                                }
                                _ => {
                                    // Ignoriere andere Events
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    _ = sleep(Duration::from_secs(2)) => {
                        println!("[Peer 2] Warte noch... (request_sent={}, chunk_received={})", 
                            request_sent_received, chunk_received);
                    }
                }
            }
            
            (request_sent_received, chunk_received)
        }).await;
        
        match result {
            Ok((request_sent, chunk_received)) => {
                println!("\n=== Ergebnis ===");
                println!("ChunkRequestSent empfangen: {}", request_sent);
                println!("ChunkReceived empfangen: {}", chunk_received);
                
                if !request_sent {
                    eprintln!("❌ PROBLEM: ChunkRequestSent wurde nicht empfangen!");
                }
                if !chunk_received {
                    eprintln!("❌ PROBLEM: ChunkReceived wurde nicht empfangen!");
                }
                
                assert!(request_sent, "ChunkRequestSent sollte empfangen worden sein");
                assert!(chunk_received, "ChunkReceived sollte empfangen worden sein");
            }
            Err(_) => {
                eprintln!("❌ Timeout: Keine Response innerhalb von 120s");
                panic!("Timeout beim Warten auf Chunk-Response (10MB)");
            }
        }
    }

    /// Test mit 4 Peers: 2 Uploader + 2 Downloader
    /// Testet parallele Transfers und ob das System mehrere gleichzeitige Downloads/Uploads handhaben kann
    /// Jeder Downloader lädt von beiden Uploadern (testet parallele Quellen)
    #[tokio::test]
    #[ignore]
    async fn test_large_chunk_transfer_10mb_4peers() {
        println!("\n=== TEST: Großer Chunk-Transfer (10MB) mit 4 Peers ===");
        println!("Peer 1 & 2: Uploader (haben den Chunk)");
        println!("Peer 3 & 4: Downloader (wollen den Chunk)");
        println!("Jeder Downloader lädt von beiden Uploadern (parallele Quellen)");
        
        // Erstelle 4 Peers
        let keypair1 = identity::Keypair::generate_ed25519();
        let peer_id1 = keypair1.public().to_peer_id();
        let peer_id1_str = peer_id1.to_string();
        
        let keypair2 = identity::Keypair::generate_ed25519();
        let peer_id2 = keypair2.public().to_peer_id();
        let peer_id2_str = peer_id2.to_string();
        
        let keypair3 = identity::Keypair::generate_ed25519();
        let peer_id3 = keypair3.public().to_peer_id();
        let peer_id3_str = peer_id3.to_string();
        
        let keypair4 = identity::Keypair::generate_ed25519();
        let peer_id4 = keypair4.public().to_peer_id();
        let peer_id4_str = peer_id4.to_string();
        
        println!("Peer 1 (Uploader) ID: {}", peer_id1_str);
        println!("Peer 2 (Uploader) ID: {}", peer_id2_str);
        println!("Peer 3 (Downloader) ID: {}", peer_id3_str);
        println!("Peer 4 (Downloader) ID: {}", peer_id4_str);
        
        // Erstelle Test-Chunk-Daten (10MB)
        let chunk_hash = "test_chunk:0";
        let chunk_size = 10 * 1024 * 1024; // 10MB
        let chunk_data: Vec<u8> = (0..chunk_size).map(|i| (i % 256) as u8).collect();
        
        // Peer 1 & 2: Haben den Chunk (Uploader)
        let chunks_data1: HashMap<String, Vec<u8>> = {
            let mut map = HashMap::new();
            map.insert(chunk_hash.to_string(), chunk_data.clone());
            map
        };
        let chunks_data2: HashMap<String, Vec<u8>> = {
            let mut map = HashMap::new();
            map.insert(chunk_hash.to_string(), chunk_data.clone());
            map
        };
        
        let chunk_loader1: ChunkLoader = Arc::new(move |hash: &str| {
            println!("[Peer 1] ChunkLoader aufgerufen für: {} ({} MB)", hash, 
                chunks_data1.get(hash).map(|d| d.len() / (1024 * 1024)).unwrap_or(0));
            chunks_data1.get(hash).cloned()
        });
        
        let chunk_loader2: ChunkLoader = Arc::new(move |hash: &str| {
            println!("[Peer 2] ChunkLoader aufgerufen für: {} ({} MB)", hash, 
                chunks_data2.get(hash).map(|d| d.len() / (1024 * 1024)).unwrap_or(0));
            chunks_data2.get(hash).cloned()
        });
        
        // Peer 3 & 4: Wollen den Chunk (Downloader)
        let chunk_loader3: ChunkLoader = Arc::new(move |_hash: &str| {
            println!("[Peer 3] ChunkLoader aufgerufen (sollte nicht passieren)");
            None
        });
        
        let chunk_loader4: ChunkLoader = Arc::new(move |_hash: &str| {
            println!("[Peer 4] ChunkLoader aufgerufen (sollte nicht passieren)");
            None
        });
        
        // Event-Channels für alle 4 Peers
        let (event_tx1, _event_rx1) = mpsc::channel::<DiscoveryEvent>(128);
        let (event_tx2, _event_rx2) = mpsc::channel::<DiscoveryEvent>(128);
        let (event_tx3, mut event_rx3) = mpsc::channel::<DiscoveryEvent>(128);
        let (event_tx4, mut event_rx4) = mpsc::channel::<DiscoveryEvent>(128);
        
        // Download-Request-Channels
        let (_download_request_tx1, download_request_rx1) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        let (_download_request_tx2, download_request_rx2) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        let (download_request_tx3, download_request_rx3) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        let (download_request_tx4, download_request_rx4) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();
        
        // Starte alle 4 Peers
        println!("\nStarte Peer 1 (Uploader)...");
        let _handle1 = start_discovery(
            event_tx1,
            Some("Peer1".to_string()),
            Some(0),
            Some(keypair1),
            None,
            None,
            Some(chunk_loader1),
            Some(download_request_rx1),
            None,
            15,
        ).await;
        
        println!("Starte Peer 2 (Uploader)...");
        let _handle2 = start_discovery(
            event_tx2,
            Some("Peer2".to_string()),
            Some(0),
            Some(keypair2),
            None,
            None,
            Some(chunk_loader2),
            Some(download_request_rx2),
            None,
            15,
        ).await;
        
        println!("Starte Peer 3 (Downloader)...");
        let _handle3 = start_discovery(
            event_tx3,
            Some("Peer3".to_string()),
            Some(0),
            Some(keypair3),
            None,
            None,
            Some(chunk_loader3),
            Some(download_request_rx3),
            None,
            15,
        ).await;
        
        println!("Starte Peer 4 (Downloader)...");
        let _handle4 = start_discovery(
            event_tx4,
            Some("Peer4".to_string()),
            Some(0),
            Some(keypair4),
            None,
            None,
            Some(chunk_loader4),
            Some(download_request_rx4),
            None,
            15,
        ).await;
        
        // Warte auf Peer-Discovery
        println!("\nWarte 5 Sekunden auf Peer-Discovery...");
        sleep(Duration::from_secs(5)).await;
        
        // Jeder Downloader lädt von beiden Uploadern (testet parallele Quellen)
        println!("\n[Peer 3] Sende Chunk-Request für: {} (10MB) von Peer 1", chunk_hash);
        download_request_tx3.send(DownloadRequest::RequestChunk {
            peer_id: peer_id1_str.clone(),
            chunk_hash: chunk_hash.to_string(),
            game_id: "test_game".to_string(),
        }).unwrap();
        
        println!("[Peer 3] Sende Chunk-Request für: {} (10MB) von Peer 2", chunk_hash);
        download_request_tx3.send(DownloadRequest::RequestChunk {
            peer_id: peer_id2_str.clone(),
            chunk_hash: chunk_hash.to_string(),
            game_id: "test_game".to_string(),
        }).unwrap();
        
        println!("[Peer 4] Sende Chunk-Request für: {} (10MB) von Peer 1", chunk_hash);
        download_request_tx4.send(DownloadRequest::RequestChunk {
            peer_id: peer_id1_str.clone(),
            chunk_hash: chunk_hash.to_string(),
            game_id: "test_game".to_string(),
        }).unwrap();
        
        println!("[Peer 4] Sende Chunk-Request für: {} (10MB) von Peer 2", chunk_hash);
        download_request_tx4.send(DownloadRequest::RequestChunk {
            peer_id: peer_id2_str.clone(),
            chunk_hash: chunk_hash.to_string(),
            game_id: "test_game".to_string(),
        }).unwrap();
        
        // Warte auf Events von beiden Downloadern
        println!("\n[Peer 3 & 4] Warte auf Chunk-Responses (10MB)...");
        println!("Erwartung: Jeder Downloader sollte von beiden Uploadern laden können");
        let result = timeout(Duration::from_secs(180), async {
            let mut peer3_chunk_received = false;
            let mut peer3_request_count = 0;
            let mut peer3_chunks_from = std::collections::HashSet::new();
            
            let mut peer4_chunk_received = false;
            let mut peer4_request_count = 0;
            let mut peer4_chunks_from = std::collections::HashSet::new();
            
            loop {
                tokio::select! {
                    event = event_rx3.recv() => {
                        if let Some(event) = event {
                            match event {
                                DiscoveryEvent::ChunkRequestSent { peer_id, chunk_hash: hash, .. } => {
                                    println!("[Peer 3] ✓ ChunkRequestSent: {} an {}", hash, peer_id);
                                    peer3_request_count += 1;
                                }
                                DiscoveryEvent::ChunkReceived { peer_id, chunk_hash: hash, chunk_data: data } => {
                                    println!("[Peer 3] ✓✓✓ ChunkReceived: {} von {} ({} MB)", 
                                        hash, peer_id, data.len() / (1024 * 1024));
                                    
                                    // Validiere Daten
                                    assert_eq!(hash, chunk_hash);
                                    assert_eq!(data.len(), chunk_size);
                                    
                                    // Validiere Dateninhalt
                                    if data.len() > 0 {
                                        let first_expected = 0u8;
                                        let last_expected = ((data.len() - 1) % 256) as u8;
                                        if data[0] != first_expected {
                                            panic!("[Peer 3] Datenfehler an Position 0: erwartet {}, erhalten {}", first_expected, data[0]);
                                        }
                                        if data[data.len() - 1] != last_expected {
                                            panic!("[Peer 3] Datenfehler an Position {}: erwartet {}, erhalten {}", data.len() - 1, last_expected, data[data.len() - 1]);
                                        }
                                    }
                                    
                                    peer3_chunks_from.insert(peer_id.clone());
                                    println!("[Peer 3] ✓✓✓ Datenvalidierung erfolgreich! (Empfangen von: {})", peer_id);
                                    peer3_chunk_received = true;
                                }
                                DiscoveryEvent::ChunkRequestFailed { chunk_hash: hash, error, .. } => {
                                    eprintln!("[Peer 3] ❌ ChunkRequestFailed: {} - {}", hash, error);
                                }
                                _ => {}
                            }
                        } else {
                            break;
                        }
                    }
                    event = event_rx4.recv() => {
                        if let Some(event) = event {
                            match event {
                                DiscoveryEvent::ChunkRequestSent { peer_id, chunk_hash: hash, .. } => {
                                    println!("[Peer 4] ✓ ChunkRequestSent: {} an {}", hash, peer_id);
                                    peer4_request_count += 1;
                                }
                                DiscoveryEvent::ChunkReceived { peer_id, chunk_hash: hash, chunk_data: data } => {
                                    println!("[Peer 4] ✓✓✓ ChunkReceived: {} von {} ({} MB)", 
                                        hash, peer_id, data.len() / (1024 * 1024));
                                    
                                    // Validiere Daten
                                    assert_eq!(hash, chunk_hash);
                                    assert_eq!(data.len(), chunk_size);
                                    
                                    // Validiere Dateninhalt
                                    if data.len() > 0 {
                                        let first_expected = 0u8;
                                        let last_expected = ((data.len() - 1) % 256) as u8;
                                        if data[0] != first_expected {
                                            panic!("[Peer 4] Datenfehler an Position 0: erwartet {}, erhalten {}", first_expected, data[0]);
                                        }
                                        if data[data.len() - 1] != last_expected {
                                            panic!("[Peer 4] Datenfehler an Position {}: erwartet {}, erhalten {}", data.len() - 1, last_expected, data[data.len() - 1]);
                                        }
                                    }
                                    
                                    peer4_chunks_from.insert(peer_id.clone());
                                    println!("[Peer 4] ✓✓✓ Datenvalidierung erfolgreich! (Empfangen von: {})", peer_id);
                                    peer4_chunk_received = true;
                                }
                                DiscoveryEvent::ChunkRequestFailed { chunk_hash: hash, error, .. } => {
                                    eprintln!("[Peer 4] ❌ ChunkRequestFailed: {} - {}", hash, error);
                                }
                                _ => {}
                            }
                        } else {
                            break;
                        }
                    }
                    _ = sleep(Duration::from_secs(2)) => {
                        println!("[Status] Peer 3: requests={}, chunk_received={}, von {} Quellen | Peer 4: requests={}, chunk_received={}, von {} Quellen", 
                            peer3_request_count, peer3_chunk_received, peer3_chunks_from.len(),
                            peer4_request_count, peer4_chunk_received, peer4_chunks_from.len());
                        
                        // Beende wenn beide Chunks empfangen wurden
                        if peer3_chunk_received && peer4_chunk_received {
                            break;
                        }
                    }
                }
            }
            
            (peer3_request_count, peer3_chunk_received, peer3_chunks_from, peer4_request_count, peer4_chunk_received, peer4_chunks_from)
        }).await;
        
        match result {
            Ok((peer3_request_count, peer3_chunk_received, peer3_chunks_from, peer4_request_count, peer4_chunk_received, peer4_chunks_from)) => {
                println!("\n=== Ergebnis ===");
                println!("Peer 3 (Downloader):");
                println!("  ChunkRequests gesendet: {} (erwartet: 2 - einer an jeden Uploader)", peer3_request_count);
                println!("  ChunkReceived: {}", peer3_chunk_received);
                println!("  Quellen: {:?}", peer3_chunks_from);
                println!("Peer 4 (Downloader):");
                println!("  ChunkRequests gesendet: {} (erwartet: 2 - einer an jeden Uploader)", peer4_request_count);
                println!("  ChunkReceived: {}", peer4_chunk_received);
                println!("  Quellen: {:?}", peer4_chunks_from);
                
                if peer3_request_count < 2 {
                    eprintln!("⚠️  WARNUNG: Peer 3 - Nur {} von 2 erwarteten Requests gesendet", peer3_request_count);
                }
                if !peer3_chunk_received {
                    eprintln!("❌ PROBLEM: Peer 3 - ChunkReceived wurde nicht empfangen!");
                }
                if peer4_request_count < 2 {
                    eprintln!("⚠️  WARNUNG: Peer 4 - Nur {} von 2 erwarteten Requests gesendet", peer4_request_count);
                }
                if !peer4_chunk_received {
                    eprintln!("❌ PROBLEM: Peer 4 - ChunkReceived wurde nicht empfangen!");
                }
                
                // Prüfe ob beide Uploader genutzt wurden (optional - könnte auch nur einer bedient werden)
                if peer3_chunks_from.len() > 0 && peer4_chunks_from.len() > 0 {
                    println!("\n✓ Beide Downloader haben den Chunk erfolgreich empfangen!");
                    if peer3_chunks_from.len() > 1 || peer4_chunks_from.len() > 1 {
                        println!("✓✓ Zusätzlich: Mindestens ein Downloader hat von mehreren Quellen geladen!");
                    }
                }
                
                assert!(peer3_chunk_received, "Peer 3: ChunkReceived sollte empfangen worden sein");
                assert!(peer4_chunk_received, "Peer 4: ChunkReceived sollte empfangen worden sein");
                
                println!("\n✓✓✓ Alle parallelen Transfers erfolgreich abgeschlossen!");
            }
            Err(_) => {
                eprintln!("❌ Timeout: Keine Response innerhalb von 180s");
                panic!("Timeout beim Warten auf Chunk-Responses (10MB) von beiden Peers");
            }
        }
    }
}

