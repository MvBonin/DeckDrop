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
}
