use libp2p::request_response::{Codec, ProtocolSupport};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::io;
use std::time::Duration;
use futures::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};

/// Request zum Abfragen der Spiele-Liste eines Peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamesListRequest;

/// Response mit der Liste der Spiele eines Peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamesListResponse {
    pub games: Vec<NetworkGameInfo>,
}

/// Spiel-Informationen für das Netzwerk (ohne lokale Pfade)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NetworkGameInfo {
    pub game_id: String,  // Eindeutige Spiel-ID zum Vergleich
    pub name: String,
    pub version: String,
    pub start_file: String,
    #[serde(default)]
    pub start_args: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub creator_peer_id: Option<String>,
}

impl NetworkGameInfo {
    /// Erstellt einen eindeutigen Schlüssel für dieses Spiel (game_id + version)
    pub fn unique_key(&self) -> String {
        format!("{}:{}", self.game_id, self.version)
    }
}

/// Codec für Game-Listen-Requests/Responses
#[derive(Clone, Default)]
pub struct GamesListCodec;

#[async_trait::async_trait]
impl Codec for GamesListCodec {
    type Protocol = String;
    type Request = GamesListRequest;
    type Response = GamesListResponse;

    async fn read_request<T>(&mut self, _protocol: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Lese Länge (4 Bytes)
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        
        // Lese Daten
        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;
        
        serde_json::from_slice(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(&mut self, _protocol: &Self::Protocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Lese Länge (4 Bytes)
        let mut len_bytes = [0u8; 4];
        io.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        
        // Lese Daten
        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;
        
        serde_json::from_slice(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(&mut self, _protocol: &Self::Protocol, io: &mut T, req: Self::Request) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let json = serde_json::to_vec(&req)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        // Schreibe Länge (4 Bytes)
        let len = json.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        
        // Schreibe Daten
        io.write_all(&json).await?;
        io.flush().await?;
        
        Ok(())
    }

    async fn write_response<T>(&mut self, _protocol: &Self::Protocol, io: &mut T, res: Self::Response) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let json = serde_json::to_vec(&res)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        // Schreibe Länge (4 Bytes)
        let len = json.len() as u32;
        io.write_all(&len.to_be_bytes()).await?;
        
        // Schreibe Daten
        io.write_all(&json).await?;
        io.flush().await?;
        
        Ok(())
    }
}

/// Network Behaviour für Game-Listen
pub type GamesListBehaviour = libp2p::request_response::Behaviour<GamesListCodec>;

/// Erstellt ein GamesListBehaviour
pub fn create_games_list_behaviour() -> GamesListBehaviour {
    let config = libp2p::request_response::Config::default()
        .with_request_timeout(Duration::from_secs(10));
    
    libp2p::request_response::Behaviour::new(
        [(String::from("/deckdrop/games-list/1.0.0"), ProtocolSupport::Full)],
        config,
    )
}

/// Event für Game-Listen
#[derive(Debug)]
pub enum GamesListEvent {
    Response {
        peer_id: PeerId,
        response: GamesListResponse,
    },
    Request {
        peer_id: PeerId,
        request: GamesListRequest,
        channel: libp2p::request_response::ResponseChannel<GamesListResponse>,
    },
    OutboundFailure {
        peer_id: PeerId,
        error: libp2p::request_response::OutboundFailure,
    },
    InboundFailure {
        peer_id: PeerId,
        error: libp2p::request_response::InboundFailure,
    },
}

impl From<libp2p::request_response::Event<GamesListRequest, GamesListResponse>> for GamesListEvent {
    fn from(event: libp2p::request_response::Event<GamesListRequest, GamesListResponse>) -> Self {
        match event {
            libp2p::request_response::Event::Message { message, peer, .. } => match message {
                libp2p::request_response::Message::Request { request, channel, .. } => {
                    GamesListEvent::Request {
                        peer_id: peer,
                        request,
                        channel,
                    }
                }
                libp2p::request_response::Message::Response { response, .. } => {
                    GamesListEvent::Response {
                        peer_id: peer,
                        response,
                    }
                }
            },
            libp2p::request_response::Event::OutboundFailure { peer, error, .. } => {
                GamesListEvent::OutboundFailure { peer_id: peer, error }
            }
            libp2p::request_response::Event::InboundFailure { peer, error, .. } => {
                GamesListEvent::InboundFailure { peer_id: peer, error }
            }
            libp2p::request_response::Event::ResponseSent { .. } => {
                // Response wurde gesendet - ignorieren für jetzt
                // Wir könnten dies später für Logging verwenden
                // Da wir Clone nicht haben, müssen wir einen anderen Ansatz verwenden
                // Für jetzt ignorieren wir ResponseSent Events
                return GamesListEvent::OutboundFailure {
                    peer_id: PeerId::from_bytes(&[]).unwrap(),
                    error: libp2p::request_response::OutboundFailure::ConnectionClosed,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::io::Cursor;

    #[test]
    fn test_games_list_request_serialization() {
        // Test: GamesListRequest sollte serialisiert werden können
        let request = GamesListRequest;
        let json = serde_json::to_string(&request).unwrap();
        
        // Deserialisiere zurück
        let deserialized: GamesListRequest = serde_json::from_str(&json).unwrap();
        // GamesListRequest ist ein Unit-Struct, also können wir nur prüfen, dass es funktioniert
        let _ = deserialized;
    }

    #[test]
    fn test_games_list_response_serialization() {
        // Test: GamesListResponse sollte korrekt serialisiert/deserialisiert werden
        let response = GamesListResponse {
            games: vec![
                NetworkGameInfo {
                    game_id: "game-1".to_string(),
                    name: "Test Game 1".to_string(),
                    version: "1.0".to_string(),
                    start_file: "game1.exe".to_string(),
                    start_args: Some("--fullscreen".to_string()),
                    description: Some("Ein Test-Spiel".to_string()),
                    creator_peer_id: Some("peer-1".to_string()),
                },
                NetworkGameInfo {
                    game_id: "game-2".to_string(),
                    name: "Test Game 2".to_string(),
                    version: "2.0".to_string(),
                    start_file: "game2.exe".to_string(),
                    start_args: None,
                    description: None,
                    creator_peer_id: None,
                },
            ],
        };
        
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: GamesListResponse = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.games.len(), 2);
        assert_eq!(deserialized.games[0].game_id, "game-1");
        assert_eq!(deserialized.games[0].name, "Test Game 1");
        assert_eq!(deserialized.games[1].game_id, "game-2");
        assert_eq!(deserialized.games[1].name, "Test Game 2");
    }

    #[test]
    fn test_network_game_info_serialization() {
        // Test: NetworkGameInfo sollte korrekt serialisiert werden
        let game_info = NetworkGameInfo {
            game_id: "test-game-id".to_string(),
            name: "My Game".to_string(),
            version: "1.5.0".to_string(),
            start_file: "start.sh".to_string(),
            start_args: Some("--debug".to_string()),
            description: Some("Ein cooles Spiel".to_string()),
            creator_peer_id: Some("creator-peer-123".to_string()),
        };
        
        let json = serde_json::to_string(&game_info).unwrap();
        assert!(json.contains("test-game-id"));
        assert!(json.contains("My Game"));
        assert!(json.contains("1.5.0"));
        
        let deserialized: NetworkGameInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.game_id, game_info.game_id);
        assert_eq!(deserialized.name, game_info.name);
        assert_eq!(deserialized.version, game_info.version);
        assert_eq!(deserialized.start_file, game_info.start_file);
        assert_eq!(deserialized.start_args, game_info.start_args);
        assert_eq!(deserialized.description, game_info.description);
        assert_eq!(deserialized.creator_peer_id, game_info.creator_peer_id);
    }

    #[test]
    fn test_network_game_info_with_optional_fields() {
        // Test: NetworkGameInfo mit optionalen Feldern
        let game_info = NetworkGameInfo {
            game_id: "minimal-game".to_string(),
            name: "Minimal Game".to_string(),
            version: "1.0".to_string(),
            start_file: "game.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        let json = serde_json::to_string(&game_info).unwrap();
        let deserialized: NetworkGameInfo = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.game_id, game_info.game_id);
        assert_eq!(deserialized.start_args, None);
        assert_eq!(deserialized.description, None);
        assert_eq!(deserialized.creator_peer_id, None);
    }

    #[tokio::test]
    async fn test_codec_write_read_request() {
        // Test: Codec sollte Request korrekt schreiben und lesen
        let mut codec = GamesListCodec;
        let protocol = String::from("/deckdrop/games-list/1.0.0");
        let request = GamesListRequest;
        
        // Schreibe Request
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        codec.write_request(&protocol, &mut cursor, request).await.unwrap();
        
        // Lese Request zurück
        let mut read_cursor = Cursor::new(&buffer);
        let read_request = codec.read_request(&protocol, &mut read_cursor).await.unwrap();
        
        // GamesListRequest ist ein Unit-Struct, also können wir nur prüfen, dass es funktioniert
        let _ = read_request;
    }

    #[tokio::test]
    async fn test_codec_write_read_response() {
        // Test: Codec sollte Response korrekt schreiben und lesen
        let mut codec = GamesListCodec;
        let protocol = String::from("/deckdrop/games-list/1.0.0");
        let response = GamesListResponse {
            games: vec![
                NetworkGameInfo {
                    game_id: "test-id".to_string(),
                    name: "Test Game".to_string(),
                    version: "1.0".to_string(),
                    start_file: "test.exe".to_string(),
                    start_args: None,
                    description: None,
                    creator_peer_id: None,
                },
            ],
        };
        
        // Schreibe Response
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        codec.write_response(&protocol, &mut cursor, response.clone()).await.unwrap();
        
        // Lese Response zurück
        let mut read_cursor = Cursor::new(&buffer);
        let read_response = codec.read_response(&protocol, &mut read_cursor).await.unwrap();
        
        assert_eq!(read_response.games.len(), 1);
        assert_eq!(read_response.games[0].game_id, response.games[0].game_id);
        assert_eq!(read_response.games[0].name, response.games[0].name);
    }

    #[tokio::test]
    async fn test_codec_write_read_response_empty() {
        // Test: Codec sollte leere Response korrekt handhaben
        let mut codec = GamesListCodec;
        let protocol = String::from("/deckdrop/games-list/1.0.0");
        let response = GamesListResponse {
            games: Vec::new(),
        };
        
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        codec.write_response(&protocol, &mut cursor, response.clone()).await.unwrap();
        
        let mut read_cursor = Cursor::new(&buffer);
        let read_response = codec.read_response(&protocol, &mut read_cursor).await.unwrap();
        
        assert_eq!(read_response.games.len(), 0);
    }

    #[tokio::test]
    async fn test_codec_write_read_response_multiple_games() {
        // Test: Codec sollte mehrere Spiele korrekt handhaben
        let mut codec = GamesListCodec;
        let protocol = String::from("/deckdrop/games-list/1.0.0");
        let response = GamesListResponse {
            games: vec![
                NetworkGameInfo {
                    game_id: "game-1".to_string(),
                    name: "Game 1".to_string(),
                    version: "1.0".to_string(),
                    start_file: "game1.exe".to_string(),
                    start_args: None,
                    description: None,
                    creator_peer_id: None,
                },
                NetworkGameInfo {
                    game_id: "game-2".to_string(),
                    name: "Game 2".to_string(),
                    version: "2.0".to_string(),
                    start_file: "game2.exe".to_string(),
                    start_args: Some("--fullscreen".to_string()),
                    description: Some("Beschreibung".to_string()),
                    creator_peer_id: Some("peer-123".to_string()),
                },
                NetworkGameInfo {
                    game_id: "game-3".to_string(),
                    name: "Game 3".to_string(),
                    version: "3.0".to_string(),
                    start_file: "game3.sh".to_string(),
                    start_args: None,
                    description: None,
                    creator_peer_id: None,
                },
            ],
        };
        
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        codec.write_response(&protocol, &mut cursor, response.clone()).await.unwrap();
        
        let mut read_cursor = Cursor::new(&buffer);
        let read_response = codec.read_response(&protocol, &mut read_cursor).await.unwrap();
        
        assert_eq!(read_response.games.len(), 3);
        assert_eq!(read_response.games[0].game_id, "game-1");
        assert_eq!(read_response.games[1].game_id, "game-2");
        assert_eq!(read_response.games[1].start_args, Some("--fullscreen".to_string()));
        assert_eq!(read_response.games[2].game_id, "game-3");
    }

    #[tokio::test]
    async fn test_codec_error_handling_invalid_json() {
        // Test: Codec sollte Fehler bei ungültigem JSON korrekt behandeln
        let mut codec = GamesListCodec;
        let protocol = String::from("/deckdrop/games-list/1.0.0");
        
        // Erstelle ungültige Daten (falsche Länge)
        let mut buffer = vec![0u8; 4];
        buffer[0] = 0xFF; // Sehr große Länge
        buffer[1] = 0xFF;
        buffer[2] = 0xFF;
        buffer[3] = 0xFF;
        
        let mut cursor = Cursor::new(&buffer);
        let result = codec.read_response(&protocol, &mut cursor).await;
        
        assert!(result.is_err());
    }

    #[test]
    fn test_create_games_list_behaviour() {
        // Test: create_games_list_behaviour sollte ein Behaviour erstellen
        let behaviour = create_games_list_behaviour();
        
        // Prüfe, dass das Behaviour erstellt wurde (keine Panic)
        let _ = behaviour;
    }

    #[test]
    fn test_network_game_info_game_id_comparison() {
        // Test: game_id sollte zum Vergleich verwendet werden können
        let game1 = NetworkGameInfo {
            game_id: "same-id".to_string(),
            name: "Game 1".to_string(),
            version: "1.0".to_string(),
            start_file: "game1.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        let game2 = NetworkGameInfo {
            game_id: "same-id".to_string(), // Gleiche ID
            name: "Game 2".to_string(), // Aber anderer Name
            version: "2.0".to_string(),
            start_file: "game2.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        let game3 = NetworkGameInfo {
            game_id: "different-id".to_string(), // Andere ID
            name: "Game 1".to_string(), // Aber gleicher Name
            version: "1.0".to_string(),
            start_file: "game1.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        // Spiele mit gleicher game_id sollten als gleich erkannt werden
        assert_eq!(game1.game_id, game2.game_id);
        assert_ne!(game1.game_id, game3.game_id);
    }

    #[test]
    fn test_games_list_response_empty() {
        // Test: Leere GamesListResponse
        let response = GamesListResponse {
            games: Vec::new(),
        };
        
        assert_eq!(response.games.len(), 0);
        
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("games"));
        
        let deserialized: GamesListResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.games.len(), 0);
    }

    #[test]
    fn test_network_game_info_all_fields() {
        // Test: NetworkGameInfo mit allen Feldern
        let game_info = NetworkGameInfo {
            game_id: "complete-game".to_string(),
            name: "Complete Game".to_string(),
            version: "1.2.3".to_string(),
            start_file: "start.bat".to_string(),
            start_args: Some("--windowed --debug".to_string()),
            description: Some("Ein vollständiges Spiel mit allen Feldern".to_string()),
            creator_peer_id: Some("creator-456".to_string()),
        };
        
        let json = serde_json::to_string(&game_info).unwrap();
        assert!(json.contains("complete-game"));
        assert!(json.contains("Complete Game"));
        assert!(json.contains("1.2.3"));
        assert!(json.contains("start.bat"));
        assert!(json.contains("--windowed --debug"));
        assert!(json.contains("Ein vollständiges Spiel"));
        assert!(json.contains("creator-456"));
    }

    #[test]
    fn test_network_game_info_unique_key() {
        // Test: unique_key sollte game_id + version kombinieren
        let game1 = NetworkGameInfo {
            game_id: "game-123".to_string(),
            name: "Test Game".to_string(),
            version: "1.0".to_string(),
            start_file: "game.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        let game2 = NetworkGameInfo {
            game_id: "game-123".to_string(),
            name: "Test Game".to_string(),
            version: "2.0".to_string(), // Andere Version
            start_file: "game.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        let game3 = NetworkGameInfo {
            game_id: "game-123".to_string(),
            name: "Test Game".to_string(),
            version: "1.0".to_string(), // Gleiche Version wie game1
            start_file: "game.exe".to_string(),
            start_args: None,
            description: None,
            creator_peer_id: None,
        };
        
        assert_eq!(game1.unique_key(), "game-123:1.0");
        assert_eq!(game2.unique_key(), "game-123:2.0");
        assert_eq!(game3.unique_key(), "game-123:1.0");
        assert_eq!(game1.unique_key(), game3.unique_key()); // Gleiche game_id + version
        assert_ne!(game1.unique_key(), game2.unique_key()); // Verschiedene Versionen
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use libp2p::identity;

    #[test]
    fn test_two_peers_game_exchange_serialization() {
        // Erstelle zwei Keypairs für die Peers (für Peer-IDs)
        let keypair1 = identity::Keypair::generate_ed25519();
        let peer_id1 = libp2p::PeerId::from(keypair1.public());
        
        let keypair2 = identity::Keypair::generate_ed25519();
        let _peer_id2 = libp2p::PeerId::from(keypair2.public());
        
        // Erstelle ein Test-Spiel für Peer 1
        let test_game = NetworkGameInfo {
            game_id: "test-game-123".to_string(),
            name: "Test Game".to_string(),
            version: "1.0".to_string(),
            start_file: "game.exe".to_string(),
            start_args: None,
            description: Some("Ein Test-Spiel".to_string()),
            creator_peer_id: Some(peer_id1.to_string()),
        };
        
        // Test: unique_key funktioniert korrekt
        assert_eq!(test_game.unique_key(), "test-game-123:1.0");
        
        // Test: Serialisierung/Deserialisierung (simuliert den Austausch zwischen Peers)
        let response = GamesListResponse {
            games: vec![test_game.clone()],
        };
        
        // Serialisiere (Peer 1 sendet)
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test-game-123"));
        assert!(json.contains("Test Game"));
        assert!(json.contains("1.0"));
        
        // Deserialisiere (Peer 2 empfängt)
        let deserialized: GamesListResponse = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.games.len(), 1);
        assert_eq!(deserialized.games[0].game_id, "test-game-123");
        assert_eq!(deserialized.games[0].name, "Test Game");
        assert_eq!(deserialized.games[0].version, "1.0");
        assert_eq!(deserialized.games[0].unique_key(), "test-game-123:1.0");
        assert_eq!(deserialized.games[0].creator_peer_id, Some(peer_id1.to_string()));
        
        // Test: Verschiedene Versionen haben verschiedene unique_keys
        let test_game_v2 = NetworkGameInfo {
            game_id: "test-game-123".to_string(),
            name: "Test Game".to_string(),
            version: "2.0".to_string(),
            start_file: "game.exe".to_string(),
            start_args: None,
            description: Some("Ein Test-Spiel v2".to_string()),
            creator_peer_id: Some(peer_id1.to_string()),
        };
        
        assert_eq!(test_game_v2.unique_key(), "test-game-123:2.0");
        assert_ne!(test_game.unique_key(), test_game_v2.unique_key());
        
        // Test: Beide Versionen können gleichzeitig existieren
        let response_both = GamesListResponse {
            games: vec![test_game.clone(), test_game_v2.clone()],
        };
        
        let json_both = serde_json::to_string(&response_both).unwrap();
        let deserialized_both: GamesListResponse = serde_json::from_str(&json_both).unwrap();
        
        assert_eq!(deserialized_both.games.len(), 2);
        assert_eq!(deserialized_both.games[0].unique_key(), "test-game-123:1.0");
        assert_eq!(deserialized_both.games[1].unique_key(), "test-game-123:2.0");
    }
}

