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
}

