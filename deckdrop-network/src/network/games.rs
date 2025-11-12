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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

