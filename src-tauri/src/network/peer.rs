use libp2p::PeerId;
use std::net::IpAddr;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: String,
    pub addr: Option<String>,
}

impl From<(PeerId, Option<IpAddr>)> for PeerInfo {
    fn from((id, addr): (PeerId, Option<IpAddr>)) -> Self {
        Self {
            id: id.to_string(),
            addr: addr.map(|ip| ip.to_string()),
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
        };
        
        assert_eq!(peer.id, "test-peer-123");
        assert_eq!(peer.addr, Some("192.168.1.100:8080".to_string()));
    }

    #[test]
    fn test_peer_info_without_addr() {
        let peer = PeerInfo {
            id: "test-peer-456".to_string(),
            addr: None,
        };
        
        assert_eq!(peer.id, "test-peer-456");
        assert_eq!(peer.addr, None);
    }

    #[test]
    fn test_peer_info_serialization() {
        let peer = PeerInfo {
            id: "test-peer-789".to_string(),
            addr: Some("10.0.0.1:9000".to_string()),
        };
        
        let serialized = serde_json::to_string(&peer).unwrap();
        let deserialized: PeerInfo = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(peer.id, deserialized.id);
        assert_eq!(peer.addr, deserialized.addr);
    }
}
