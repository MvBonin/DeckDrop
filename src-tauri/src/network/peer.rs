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
}
