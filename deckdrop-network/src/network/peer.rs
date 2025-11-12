use libp2p::PeerId;
use std::net::IpAddr;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub addr: Option<String>,
    pub player_name: Option<String>,
}

impl From<(PeerId, Option<IpAddr>)> for PeerInfo {
    fn from((id, addr): (PeerId, Option<IpAddr>)) -> Self {
        Self {
            id: id.to_string(),
            addr: addr.map(|ip| ip.to_string()),
            player_name: None, // Will be populated later
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_info_creation() {
        let peer = PeerInfo {
            id: "test-peer-123".to_string(),
            addr: Some("192.168.1.100:8080".to_string()),
            player_name: Some("TestPlayer".to_string()),
        };
        
        assert_eq!(peer.id, "test-peer-123");
        assert_eq!(peer.addr, Some("192.168.1.100:8080".to_string()));
        assert_eq!(peer.player_name, Some("TestPlayer".to_string()));
    }

    #[test]
    fn test_peer_info_without_addr() {
        let peer = PeerInfo {
            id: "test-peer-456".to_string(),
            addr: None,
            player_name: None,
        };
        
        assert_eq!(peer.id, "test-peer-456");
        assert_eq!(peer.addr, None);
        assert_eq!(peer.player_name, None);
    }

    #[test]
    fn test_peer_info_serialization() {
        let peer = PeerInfo {
            id: "test-peer-789".to_string(),
            addr: Some("10.0.0.1:9000".to_string()),
            player_name: Some("SerialPlayer".to_string()),
        };
        
        let serialized = serde_json::to_string(&peer).unwrap();
        let deserialized: PeerInfo = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(peer.id, deserialized.id);
        assert_eq!(peer.addr, deserialized.addr);
        assert_eq!(peer.player_name, deserialized.player_name);
    }

    #[test]
    fn test_peer_info_from_tuple_ipv4() {
        let peer_id = PeerId::random();
        let ip = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 100));
        
        let peer_info = PeerInfo::from((peer_id.clone(), Some(ip)));
        
        assert_eq!(peer_info.id, peer_id.to_string());
        assert_eq!(peer_info.addr, Some("192.168.1.100".to_string()));
        assert_eq!(peer_info.player_name, None);
    }

    #[test]
    fn test_peer_info_from_tuple_ipv6() {
        let peer_id = PeerId::random();
        let ip = IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        
        let peer_info = PeerInfo::from((peer_id.clone(), Some(ip)));
        
        assert_eq!(peer_info.id, peer_id.to_string());
        assert_eq!(peer_info.addr, Some("2001:db8::1".to_string()));
        assert_eq!(peer_info.player_name, None);
    }

    #[test]
    fn test_peer_info_from_tuple_no_ip() {
        let peer_id = PeerId::random();
        
        let peer_info = PeerInfo::from((peer_id.clone(), None));
        
        assert_eq!(peer_info.id, peer_id.to_string());
        assert_eq!(peer_info.addr, None);
        assert_eq!(peer_info.player_name, None);
    }

    #[test]
    fn test_peer_info_serialization_without_fields() {
        let peer = PeerInfo {
            id: "minimal-peer".to_string(),
            addr: None,
            player_name: None,
        };
        
        let serialized = serde_json::to_string(&peer).unwrap();
        let deserialized: PeerInfo = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(peer.id, deserialized.id);
        assert_eq!(peer.addr, deserialized.addr);
        assert_eq!(peer.player_name, deserialized.player_name);
    }

    #[test]
    fn test_peer_info_serialization_partial() {
        // Test with only id and addr, no player_name
        let peer = PeerInfo {
            id: "partial-peer".to_string(),
            addr: Some("192.168.1.50".to_string()),
            player_name: None,
        };
        
        let serialized = serde_json::to_string(&peer).unwrap();
        let deserialized: PeerInfo = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(peer.id, deserialized.id);
        assert_eq!(peer.addr, deserialized.addr);
        assert_eq!(peer.player_name, deserialized.player_name);
    }

    #[test]
    fn test_peer_info_clone() {
        let peer = PeerInfo {
            id: "clone-test".to_string(),
            addr: Some("192.168.1.200".to_string()),
            player_name: Some("ClonePlayer".to_string()),
        };
        
        let cloned = peer.clone();
        
        assert_eq!(peer.id, cloned.id);
        assert_eq!(peer.addr, cloned.addr);
        assert_eq!(peer.player_name, cloned.player_name);
    }

    #[test]
    fn test_peer_info_debug() {
        let peer = PeerInfo {
            id: "debug-test".to_string(),
            addr: Some("192.168.1.201".to_string()),
            player_name: Some("DebugPlayer".to_string()),
        };
        
        // Should not panic when formatting
        let debug_str = format!("{:?}", peer);
        assert!(debug_str.contains("debug-test"));
    }

    #[test]
    fn test_peer_info_with_empty_strings() {
        let peer = PeerInfo {
            id: "".to_string(),
            addr: Some("".to_string()),
            player_name: Some("".to_string()),
        };
        
        assert_eq!(peer.id, "");
        assert_eq!(peer.addr, Some("".to_string()));
        assert_eq!(peer.player_name, Some("".to_string()));
    }

    #[test]
    fn test_peer_info_with_long_strings() {
        let long_id = "a".repeat(1000);
        let long_addr = "192.168.1.".repeat(100);
        let long_name = "Player".repeat(100);
        
        let peer = PeerInfo {
            id: long_id.clone(),
            addr: Some(long_addr.clone()),
            player_name: Some(long_name.clone()),
        };
        
        assert_eq!(peer.id, long_id);
        assert_eq!(peer.addr, Some(long_addr));
        assert_eq!(peer.player_name, Some(long_name));
    }

    #[test]
    fn test_peer_info_roundtrip_serialization() {
        let original = PeerInfo {
            id: "roundtrip-test".to_string(),
            addr: Some("10.20.30.40".to_string()),
            player_name: Some("RoundTripPlayer".to_string()),
        };
        
        // Serialize
        let json = serde_json::to_string(&original).unwrap();
        
        // Deserialize
        let deserialized: PeerInfo = serde_json::from_str(&json).unwrap();
        
        // Serialize again
        let json2 = serde_json::to_string(&deserialized).unwrap();
        
        // Should be identical
        assert_eq!(json, json2);
        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.addr, deserialized.addr);
        assert_eq!(original.player_name, deserialized.player_name);
    }
}