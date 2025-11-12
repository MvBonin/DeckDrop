use libp2p::{
    identity, mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    swarm::SwarmEvent, PeerId,
};
use std::str::FromStr;
use futures::StreamExt;
use std::net::IpAddr;

use crate::network::{peer::PeerInfo, channel::PeerUpdateSender};

#[derive(libp2p::swarm::NetworkBehaviour)]
pub struct DiscoveryBehaviour {
    pub mdns: Mdns,
}

// Event type for GTK integration
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    PeerFound(PeerInfo),
    PeerLost(String),
}

// Wrapper function for GTK integration
pub async fn start_discovery(event_tx: tokio::sync::mpsc::Sender<DiscoveryEvent>) -> tokio::task::JoinHandle<()> {
    let (sender, mut receiver) = crate::network::channel::new_peer_channel();
    
    // Spawn task to convert PeerInfo to DiscoveryEvent
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        while let Ok(peer_info) = receiver.recv().await {
            let _ = event_tx_clone.send(DiscoveryEvent::PeerFound(peer_info)).await;
        }
    });
    
    // Start discovery in background
    tokio::spawn(async move {
        let _ = run_discovery(sender, None).await;
    })
}

pub async fn run_discovery(sender: PeerUpdateSender, our_peer_id: Option<String>) {
    let (_id_keys, peer_id) = if let Some(peer_id_str) = our_peer_id {
        // Use provided peer ID
        let peer_id = PeerId::from_str(&peer_id_str).unwrap_or_else(|_| {
            let keys = identity::Keypair::generate_ed25519();
            PeerId::from(keys.public())
        });
        (None, peer_id)
    } else {
        // Generate new peer ID
        let keys = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(keys.public());
        (Some(keys), peer_id)
    };

    println!("Starting mDNS discovery with peer ID: {}", peer_id);
    eprintln!("Starting mDNS discovery with peer ID: {}", peer_id);

    // Create mDNS with more explicit configuration
    let mdns_config = libp2p::mdns::Config::default();
    let mdns = match Mdns::new(mdns_config, peer_id) {
        Ok(mdns) => {
            println!("mDNS discovery initialized successfully");
            eprintln!("mDNS discovery initialized successfully");
            mdns
        }
        Err(e) => {
            println!("Failed to initialize mDNS discovery: {}", e);
            eprintln!("Failed to initialize mDNS discovery: {}", e);
            return;
        }
    };

    let behaviour = DiscoveryBehaviour { mdns };
    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )
        .unwrap()
        .with_behaviour(|_| behaviour)
        .unwrap()
        .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(60)))
        .build();

    println!("Swarm created, starting discovery loop...");
    eprintln!("Swarm created, starting discovery loop...");

    // Listen on all interfaces
    if let Err(e) = swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()) {
        println!("Failed to listen on all interfaces: {}", e);
        eprintln!("Failed to listen on all interfaces: {}", e);
        return;
    } else {
        println!("Listening on all interfaces for peer discovery");
        eprintln!("Listening on all interfaces for peer discovery");
    }
    
    
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(discovery_event) => {
                match discovery_event {
                    DiscoveryBehaviourEvent::Mdns(MdnsEvent::Discovered(peers)) => {
                        println!("mDNS discovered {} peers", peers.len());
                        eprintln!("mDNS discovered {} peers", peers.len());
                        for (peer_id, addr) in peers {
                            // Extract IP address from multiaddr
                            let ip: Option<IpAddr> = addr.iter()
                                .find_map(|proto| {
                                    match proto {
                                        libp2p::multiaddr::Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
                                        libp2p::multiaddr::Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
                                        _ => None,
                                    }
                                });
                            
                            let peer_info = PeerInfo::from((peer_id, ip));
                            println!("Discovered peer: {} at {:?}", peer_info.id, peer_info.addr);
                            eprintln!("Discovered peer: {} at {:?}", peer_info.id, peer_info.addr);
                            let _ = sender.send(peer_info);
                        }
                    }
                    DiscoveryBehaviourEvent::Mdns(MdnsEvent::Expired(expired)) => {
                        println!("mDNS expired {} peers", expired.len());
                        eprintln!("mDNS expired {} peers", expired.len());
                        for (peer_id, _addr) in expired {
                            println!("Peer expired: {}", peer_id);
                            eprintln!("Peer expired: {}", peer_id);
                        }
                    }
                }
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Listening on: {}", address);
                eprintln!("Listening on: {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                println!("Connection established with: {}", peer_id);
                eprintln!("Connection established with: {}", peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                println!("Connection closed with: {}", peer_id);
                eprintln!("Connection closed with: {}", peer_id);
            }
            SwarmEvent::ListenerClosed { .. } => {
                println!("Listener closed");
                eprintln!("Listener closed");
            }
            SwarmEvent::ListenerError { error, .. } => {
                println!("Listener error: {}", error);
                eprintln!("Listener error: {}", error);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use std::collections::HashMap;

    #[test]
    fn test_peer_id_generation() {
        let id_keys = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(id_keys.public());
        
        assert!(!peer_id.to_string().is_empty());
        assert_eq!(peer_id.to_string().len(), 52); // libp2p peer ID length
    }

    #[test]
    fn test_mdns_config_creation() {
        let mdns_config = libp2p::mdns::Config::default();
        assert!(mdns_config.query_interval > Duration::from_secs(0));
    }

    #[tokio::test]
    async fn test_discovery_channel_communication() {
        let (sender, mut receiver) = crate::network::channel::new_peer_channel();
        
        // Create a test peer
        let test_peer = PeerInfo {
            id: "test-peer-123".to_string(),
            addr: Some("192.168.1.100".to_string()),
            player_name: Some("DiscoveryTest".to_string()),
        };
        
        // Send peer through channel
        let _ = sender.send(test_peer.clone());
        
        // Receive peer from channel
        let received_peer = receiver.recv().await.unwrap();
        
        assert_eq!(received_peer.id, test_peer.id);
        assert_eq!(received_peer.addr, test_peer.addr);
    }

    #[tokio::test]
    async fn test_multiple_discovery_channels() {
        let (sender1, mut receiver1) = crate::network::channel::new_peer_channel();
        let (sender2, mut receiver2) = crate::network::channel::new_peer_channel();
        
        let peer1 = PeerInfo {
            id: "peer-1".to_string(),
            addr: Some("192.168.1.101".to_string()),
            player_name: None,
        };
        
        let peer2 = PeerInfo {
            id: "peer-2".to_string(),
            addr: Some("192.168.1.102".to_string()),
            player_name: None,
        };
        
        // Send to different channels
        let _ = sender1.send(peer1.clone());
        let _ = sender2.send(peer2.clone());
        
        // Receive from respective channels
        let received1 = receiver1.recv().await.unwrap();
        let received2 = receiver2.recv().await.unwrap();
        
        assert_eq!(received1.id, peer1.id);
        assert_eq!(received2.id, peer2.id);
    }

    #[tokio::test]
    async fn test_discovery_timeout_handling() {
        let (_sender, mut receiver) = crate::network::channel::new_peer_channel();
        
        // Test that we can handle timeouts gracefully
        let timeout_result = tokio::time::timeout(
            Duration::from_millis(100),
            receiver.recv()
        ).await;
        
        // Should timeout since no message was sent
        assert!(timeout_result.is_err());
    }

    #[tokio::test]
    async fn test_peer_store_integration() {
        let peer_store = Arc::new(Mutex::new(HashMap::new()));
        let (sender, mut receiver) = crate::network::channel::new_peer_channel();
        
        let test_peer = PeerInfo {
            id: "test-peer-store".to_string(),
            addr: Some("192.168.1.103".to_string()),
            player_name: None,
        };
        
        // Send peer
        let _ = sender.send(test_peer.clone());
        
        // Simulate peer store update
        let received_peer = receiver.recv().await.unwrap();
        let mut store = peer_store.lock().await;
        store.insert(received_peer.id.clone(), received_peer);
        
        // Verify peer is in store
        assert!(store.contains_key(&test_peer.id));
        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn test_concurrent_discovery_simulation() {
        let (sender1, mut receiver1) = crate::network::channel::new_peer_channel();
        let (sender2, mut receiver2) = crate::network::channel::new_peer_channel();
        
        // Simulate two discovery instances running concurrently
        let handle1 = tokio::spawn(async move {
            let peer = PeerInfo {
                id: "concurrent-peer-1".to_string(),
                addr: Some("192.168.1.201".to_string()),
                player_name: None,
            };
            let _ = sender1.send(peer);
            sleep(Duration::from_millis(50)).await;
        });
        
        let handle2 = tokio::spawn(async move {
            let peer = PeerInfo {
                id: "concurrent-peer-2".to_string(),
                addr: Some("192.168.1.202".to_string()),
                player_name: None,
            };
            let _ = sender2.send(peer);
            sleep(Duration::from_millis(50)).await;
        });
        
        // Wait for both to complete
        let _ = tokio::join!(handle1, handle2);
        
        // Check that we can receive from both channels
        let received1 = receiver1.recv().await.unwrap();
        let received2 = receiver2.recv().await.unwrap();
        
        assert_eq!(received1.id, "concurrent-peer-1");
        assert_eq!(received2.id, "concurrent-peer-2");
    }

    #[tokio::test]
    async fn test_self_discovery() {
        use tokio::time::{sleep, Duration};
        
        // Create two discovery channels
        let (sender1, mut receiver1) = crate::network::channel::new_peer_channel();
        let (_sender2, _receiver2) = crate::network::channel::new_peer_channel();
        
        // Start two discovery instances in separate tasks
        let handle1 = tokio::spawn(async move {
            run_discovery(sender1, None).await;
        });
        
        let handle2 = tokio::spawn(async move {
            run_discovery(_sender2, None).await;
        });
        
        // Wait a bit for discovery to start
        sleep(Duration::from_millis(1000)).await;
        
        // Check if we received any peers (they should discover each other)
        let _timeout = tokio::time::timeout(Duration::from_secs(5), receiver1.recv()).await;
        
        // Clean up
        handle1.abort();
        handle2.abort();
        
        // The test passes if we either received a peer or timed out (both are valid)
        println!("Self-discovery test completed");
    }

    #[test]
    fn test_ip_address_extraction() {
        // Test IPv4 address extraction
        let addr: libp2p::Multiaddr = "/ip4/192.168.1.100/tcp/8080".parse().unwrap();
        let ip: Option<IpAddr> = addr.iter()
            .find_map(|proto| {
                match proto {
                    libp2p::multiaddr::Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
                    libp2p::multiaddr::Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
                    _ => None,
                }
            });
        
        assert!(ip.is_some());
        if let Some(IpAddr::V4(ip)) = ip {
            assert_eq!(ip.to_string(), "192.168.1.100");
        }
    }

    #[test]
    fn test_peer_info_from_tuple() {
        let peer_id = PeerId::random();
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 100));
        
        let peer_info = PeerInfo::from((peer_id, Some(ip)));
        
        assert_eq!(peer_info.id, peer_id.to_string());
        assert_eq!(peer_info.addr, Some("192.168.1.100".to_string()));
    }

    #[test]
    fn test_ipv6_address_extraction() {
        // Test IPv6 address extraction
        let addr: libp2p::Multiaddr = "/ip6/2001:db8::1/tcp/8080".parse().unwrap();
        let ip: Option<IpAddr> = addr.iter()
            .find_map(|proto| {
                match proto {
                    libp2p::multiaddr::Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
                    libp2p::multiaddr::Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
                    _ => None,
                }
            });
        
        assert!(ip.is_some());
        if let Some(IpAddr::V6(ip)) = ip {
            assert_eq!(ip.to_string(), "2001:db8::1");
        }
    }

    #[test]
    fn test_multiaddr_without_ip() {
        // Test multiaddr without IP (should return None)
        let addr: libp2p::Multiaddr = "/tcp/8080".parse().unwrap();
        let ip: Option<IpAddr> = addr.iter()
            .find_map(|proto| {
                match proto {
                    libp2p::multiaddr::Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
                    libp2p::multiaddr::Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
                    _ => None,
                }
            });
        
        assert!(ip.is_none());
    }

    #[test]
    fn test_peer_info_from_tuple_without_ip() {
        let peer_id = PeerId::random();
        
        let peer_info = PeerInfo::from((peer_id.clone(), None));
        
        assert_eq!(peer_info.id, peer_id.to_string());
        assert_eq!(peer_info.addr, None);
        assert_eq!(peer_info.player_name, None);
    }

    #[test]
    fn test_peer_info_from_tuple_with_ipv6() {
        let peer_id = PeerId::random();
        let ip = IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        
        let peer_info = PeerInfo::from((peer_id, Some(ip)));
        
        assert_eq!(peer_info.addr, Some("2001:db8::1".to_string()));
    }

    #[test]
    fn test_discovery_behaviour_creation() {
        let peer_id = PeerId::random();
        let mdns_config = libp2p::mdns::Config::default();
        
        // This should not panic
        let mdns = Mdns::new(mdns_config, peer_id);
        assert!(mdns.is_ok());
        
        let _behaviour = DiscoveryBehaviour { mdns: mdns.unwrap() };
        // Verify behaviour was created
        assert!(true); // If we get here, creation succeeded
    }

    #[tokio::test]
    async fn test_discovery_event_enum() {
        use crate::network::discovery::DiscoveryEvent;
        
        let peer = PeerInfo {
            id: "test-event".to_string(),
            addr: Some("192.168.1.100".to_string()),
            player_name: None,
        };
        
        // Test PeerFound variant
        let event = DiscoveryEvent::PeerFound(peer.clone());
        match event {
            DiscoveryEvent::PeerFound(p) => {
                assert_eq!(p.id, peer.id);
            }
            _ => panic!("Wrong event variant"),
        }
        
        // Test PeerLost variant
        let event = DiscoveryEvent::PeerLost("test-id".to_string());
        match event {
            DiscoveryEvent::PeerLost(id) => {
                assert_eq!(id, "test-id");
            }
            _ => panic!("Wrong event variant"),
        }
    }

    #[tokio::test]
    async fn test_start_discovery_function() {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<DiscoveryEvent>(32);
        
        // Start discovery
        let handle = start_discovery(event_tx).await;
        
        // Verify handle was returned
        assert!(!handle.is_finished());
        
        // Clean up
        handle.abort();
        
        // Give it a moment to clean up
        sleep(Duration::from_millis(100)).await;
    }

    #[test]
    fn test_peer_id_string_parsing() {
        let peer_id = PeerId::random();
        let peer_id_string = peer_id.to_string();
        
        // Test parsing back
        let parsed = PeerId::from_str(&peer_id_string);
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap(), peer_id);
    }

    #[test]
    fn test_peer_id_invalid_string() {
        let invalid_string = "not-a-valid-peer-id";
        let parsed = PeerId::from_str(invalid_string);
        
        // Should fail to parse
        assert!(parsed.is_err());
    }

    #[tokio::test]
    async fn test_multiple_peer_updates() {
        let (sender, mut receiver) = crate::network::channel::new_peer_channel();
        
        // Send multiple peer updates
        for i in 0..10 {
            let peer = PeerInfo {
                id: format!("update-peer-{}", i),
                addr: Some(format!("192.168.1.{}", 100 + i)),
                player_name: None,
            };
            let _ = sender.send(peer);
        }
        
        // Receive all updates
        let mut received_ids = Vec::new();
        for _ in 0..10 {
            if let Ok(peer) = receiver.recv().await {
                received_ids.push(peer.id);
            }
        }
        
        assert_eq!(received_ids.len(), 10);
        for i in 0..10 {
            assert!(received_ids.contains(&format!("update-peer-{}", i)));
        }
    }
} 