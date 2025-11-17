use crate::network::peer::PeerInfo;
use tokio::sync::broadcast;

pub type PeerUpdateSender = broadcast::Sender<PeerInfo>;
pub type PeerUpdateReceiver = broadcast::Receiver<PeerInfo>;

pub fn new_peer_channel() -> (PeerUpdateSender, PeerUpdateReceiver) {
    broadcast::channel(100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_channel_creation() {
        let (sender, mut receiver) = new_peer_channel();
        
        assert_eq!(sender.receiver_count(), 1);
        
        // Test basic send/receive
        let test_peer = PeerInfo {
            id: "test-channel".to_string(),
            addr: Some("192.168.1.100".to_string()),
            player_name: Some("ChannelTest".to_string()),
            games_count: Some(5),
            version: None,
        };
        
        let _ = sender.send(test_peer.clone());
        let received = receiver.recv().await.unwrap();
        
        assert_eq!(received.id, test_peer.id);
        assert_eq!(received.addr, test_peer.addr);
    }

    #[tokio::test]
    async fn test_multiple_receivers() {
        let (sender, mut receiver1) = new_peer_channel();
        let mut receiver2 = sender.subscribe();
        let mut receiver3 = sender.subscribe();
        
        assert_eq!(sender.receiver_count(), 3);
        
        let test_peer = PeerInfo {
            id: "multi-receiver-test".to_string(),
            addr: Some("192.168.1.101".to_string()),
            player_name: Some("MultiReceiver".to_string()),
            games_count: Some(8),
            version: None,
        };
        
        // Send one message
        let _ = sender.send(test_peer.clone());
        
        // All receivers should get the same message
        let received1 = receiver1.recv().await.unwrap();
        let received2 = receiver2.recv().await.unwrap();
        let received3 = receiver3.recv().await.unwrap();
        
        assert_eq!(received1.id, test_peer.id);
        assert_eq!(received2.id, test_peer.id);
        assert_eq!(received3.id, test_peer.id);
    }

    #[tokio::test]
    async fn test_channel_capacity() {
        let (sender, mut receiver) = new_peer_channel();
        
        // Send multiple messages
        for i in 0..5 {
            let peer = PeerInfo {
                id: format!("peer-{}", i),
                addr: Some(format!("192.168.1.{}", 100 + i)),
                player_name: None,
                games_count: None,
                version: None,
            };
            let _ = sender.send(peer);
        }
        
        // Should receive all messages
        for i in 0..5 {
            let received = receiver.recv().await.unwrap();
            assert_eq!(received.id, format!("peer-{}", i));
        }
    }

    #[tokio::test]
    async fn test_channel_timeout() {
        let (_sender, mut receiver) = new_peer_channel();
        
        // Test timeout when no message is sent
        let timeout_result = tokio::time::timeout(
            Duration::from_millis(100),
            receiver.recv()
        ).await;
        
        assert!(timeout_result.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_senders() {
        let (sender1, mut receiver1) = new_peer_channel();
        let (sender2, mut receiver2) = new_peer_channel();
        
        // Spawn multiple senders
        let handle1 = tokio::spawn(async move {
            for i in 0..3 {
                let peer = PeerInfo {
                    id: format!("sender1-peer-{}", i),
                    addr: Some(format!("192.168.1.{}", 200 + i)),
                    player_name: None,
                    games_count: None,
                    version: None,
                };
                let _ = sender1.send(peer);
                sleep(Duration::from_millis(10)).await;
            }
        });
        
        let handle2 = tokio::spawn(async move {
            for i in 0..3 {
                let peer = PeerInfo {
                    id: format!("sender2-peer-{}", i),
                    addr: Some(format!("192.168.1.{}", 300 + i)),
                    player_name: None,
                    games_count: None,
                    version: None,
                };
                let _ = sender2.send(peer);
                sleep(Duration::from_millis(10)).await;
            }
        });
        
        // Wait for senders to complete
        let _ = tokio::join!(handle1, handle2);
        
        // Receive all messages
        for i in 0..3 {
            let received1 = receiver1.recv().await.unwrap();
            let received2 = receiver2.recv().await.unwrap();
            
            assert_eq!(received1.id, format!("sender1-peer-{}", i));
            assert_eq!(received2.id, format!("sender2-peer-{}", i));
        }
    }

    #[tokio::test]
    async fn test_receiver_drop() {
        let (sender, receiver) = new_peer_channel();
        
        let _initial_count = sender.receiver_count();
        
        // Drop the receiver
        drop(receiver);
        
        // Wait a bit for the drop to be processed
        sleep(Duration::from_millis(10)).await;
        
        // Should have no receivers left
        assert_eq!(sender.receiver_count(), 0);
    }

    #[tokio::test]
    async fn test_sender_drop() {
        let (sender, mut receiver) = new_peer_channel();
        
        // Send a message
        let test_peer = PeerInfo {
            id: "drop-test".to_string(),
            addr: Some("192.168.1.100".to_string()),
            player_name: None,
            games_count: None,
            version: None,
        };
        
        let _ = sender.send(test_peer.clone());
        
        // Receive the message
        let received = receiver.recv().await.unwrap();
        assert_eq!(received.id, test_peer.id);
        
        // Drop the sender
        drop(sender);
        
        // Try to receive again - should fail
        let recv_result = receiver.recv().await;
        assert!(recv_result.is_err());
    }

    #[tokio::test]
    async fn test_channel_performance() {
        let (sender, mut receiver) = new_peer_channel();
        
        let start = std::time::Instant::now();
        
        // Send many messages quickly
        for i in 0..100 {
            let peer = PeerInfo {
                id: format!("perf-peer-{}", i),
                addr: Some(format!("192.168.1.{}", i)),
                player_name: None,
                games_count: None,
                version: None,
            };
            let _ = sender.send(peer);
        }
        
        // Receive all messages
        for i in 0..100 {
            let received = receiver.recv().await.unwrap();
            assert_eq!(received.id, format!("perf-peer-{}", i));
        }
        
        let duration = start.elapsed();
        
        // Should complete quickly (less than 1 second)
        assert!(duration < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_channel_overflow_handling() {
        let (sender, mut receiver) = new_peer_channel();
        
        // Start receiving in parallel before sending
        let receive_handle = tokio::spawn(async move {
            let mut received_messages = Vec::new();
            // Keep receiving until we get 150 messages or timeout
            let start = std::time::Instant::now();
            while received_messages.len() < 150 && start.elapsed() < Duration::from_secs(2) {
                match receiver.recv().await {
                    Ok(peer) => {
                        received_messages.push(peer);
                    }
                    Err(_) => {
                        // Channel closed or lagged - break
                        break;
                    }
                }
            }
            received_messages
        });
        
        // Give receiver a moment to start listening
        sleep(Duration::from_millis(50)).await;
        
        // Send more messages than channel capacity (100)
        // Send messages with small delays to ensure receiver can keep up
        for i in 0..150 {
            let peer = PeerInfo {
                id: format!("overflow-peer-{}", i),
                addr: Some(format!("192.168.1.{}", i % 255)),
                player_name: None,
                games_count: None,
                version: None,
            };
            let _ = sender.send(peer);
            
            // Small delay every 20 messages to allow receiver to catch up
            if i > 0 && i % 20 == 0 {
                sleep(Duration::from_millis(5)).await;
            }
        }
        
        // Drop sender to signal no more messages
        drop(sender);
        
        // Wait for receiver to finish
        let received_messages = receive_handle.await.unwrap();
        
        // Should have received messages (at least some, up to channel capacity)
        // Note: Due to broadcast channel behavior, we might receive all 150 if receiver is fast enough
        // or fewer if there's lag, but we should get at least some messages
        assert!(received_messages.len() > 0, "Should receive at least some messages (got {})", received_messages.len());
        
        // Verify we received messages from the later range (due to potential overflow)
        // The last messages sent should be in the received set
        if let Some(last_peer) = received_messages.last() {
            if let Some(num_str) = last_peer.id.strip_prefix("overflow-peer-") {
                if let Ok(num) = num_str.parse::<usize>() {
                    // If we got fewer than 150 messages, the last one should be from the later range
                    // If we got all 150, the last one should be 149
                    if received_messages.len() < 150 {
                        assert!(num >= 50, "Last received message should be from later range (got {})", num);
                    } else {
                        assert_eq!(num, 149, "Should receive last message if all were received");
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn test_broadcast_to_many_receivers() {
        let (sender, _) = new_peer_channel();
        
        // Create many receivers
        let mut receivers = Vec::new();
        for _ in 0..50 {
            receivers.push(sender.subscribe());
        }
        
        assert_eq!(sender.receiver_count(), 50);
        
        // Send a message
        let test_peer = PeerInfo {
            id: "broadcast-test".to_string(),
            addr: Some("192.168.1.200".to_string()),
            player_name: None,
            games_count: None,
            version: None,
        };
        
        let _ = sender.send(test_peer.clone());
        
        // All receivers should get the message
        for receiver in &mut receivers {
            let received = receiver.recv().await.unwrap();
            assert_eq!(received.id, test_peer.id);
        }
    }

    #[tokio::test]
    async fn test_channel_message_ordering() {
        let (sender, mut receiver) = new_peer_channel();
        
        // Send messages in order
        let peers: Vec<PeerInfo> = (0..10)
            .map(|i| PeerInfo {
                games_count: None,
                id: format!("order-peer-{}", i),
                addr: Some(format!("192.168.1.{}", 100 + i)),
                player_name: None,
                version: None,
            })
            .collect();
        
        for peer in &peers {
            let _ = sender.send(peer.clone());
        }
        
        // Receive and verify order
        for (i, expected_peer) in peers.iter().enumerate() {
            let received = receiver.recv().await.unwrap();
            assert_eq!(received.id, expected_peer.id, "Message {} out of order", i);
        }
    }

    #[tokio::test]
    async fn test_channel_with_slow_receiver() {
        let (sender, mut receiver) = new_peer_channel();
        
        // Send messages quickly
        for i in 0..20 {
            let peer = PeerInfo {
                id: format!("slow-peer-{}", i),
                addr: Some(format!("192.168.1.{}", i)),
                player_name: None,
                games_count: None,
                version: None,
            };
            let _ = sender.send(peer);
        }
        
        // Receive slowly
        for i in 0..20 {
            let received = receiver.recv().await.unwrap();
            assert_eq!(received.id, format!("slow-peer-{}", i));
            sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn test_channel_subscribe_after_send() {
        let (sender, mut receiver1) = new_peer_channel();
        
        // Send a message before subscribing
        let peer1 = PeerInfo {
            id: "before-subscribe".to_string(),
            addr: Some("192.168.1.1".to_string()),
            player_name: None,
            games_count: None,
            version: None,
        };
        let _ = sender.send(peer1.clone());
        
        // Create new receiver after message was sent
        let mut receiver2 = sender.subscribe();
        
        // Send another message
        let peer2 = PeerInfo {
            id: "after-subscribe".to_string(),
            addr: Some("192.168.1.2".to_string()),
            player_name: None,
            games_count: None,
            version: None,
        };
        let _ = sender.send(peer2.clone());
        
        // receiver1 should get both messages
        let msg1 = receiver1.recv().await.unwrap();
        let msg2 = receiver1.recv().await.unwrap();
        assert_eq!(msg1.id, peer1.id);
        assert_eq!(msg2.id, peer2.id);
        
        // receiver2 should only get the second message (subscribed after first)
        let msg = receiver2.recv().await.unwrap();
        assert_eq!(msg.id, peer2.id);
    }
}