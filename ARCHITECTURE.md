# DeckDrop Architektur

## Übersicht

DeckDrop ist eine Peer-to-Peer-Spiele-Sharing-Plattform, die aus drei Hauptkomponenten besteht:

1. **deckdrop-core**: Core-Logik ohne UI-Abhängigkeiten
2. **deckdrop-ui**: Iced-basierte Benutzeroberfläche
3. **deckdrop-network**: Netzwerk-Funktionalität (libp2p)

## Architektur-Diagramm

```
┌─────────────────────────────────────────────────────────────┐
│                     deckdrop-ui                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   app.rs     │  │  main.rs     │  │network_bridge│     │
│  │  (Iced UI)   │  │  (Entry)     │  │  (Tokio ↔   │     │
│  │              │  │              │  │   Iced)      │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
                            │
                            │ (Funktionsaufrufe)
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                   deckdrop-core                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
│  │ config.rs│  │ game.rs  │  │synch.rs  │  │gamechecker│  │
│  │          │  │          │  │          │  │    .rs     │  │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘  │
└─────────────────────────────────────────────────────────────┘
                            │
                            │ (Netzwerk-Requests)
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                 deckdrop-network                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │ discovery.rs │  │  games.rs    │  │  peer.rs     │     │
│  │  (mDNS,     │  │  (Metadata,  │  │  (Peer-Info) │     │
│  │   Events)   │  │   Chunks)    │  │              │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
                            │
                            │ (libp2p)
                            ▼
                    ┌───────────────┐
                    │   Network     │
                    │  (P2P Peers)  │
                    └───────────────┘
```

## Komponenten-Details

### deckdrop-core

**Zweck**: UI-unabhängige Core-Logik

**Module**:
- `config.rs`: Konfigurationsverwaltung (Spielername, Download-Pfad, Game-Pfade)
- `game.rs`: Spiel-Verwaltung (GameInfo, Chunk-Generierung, TOML-Parsing)
- `synch.rs`: Download-Synchronisation (Manifest, Chunk-Management, Datei-Rekonstruktion)
- `gamechecker.rs`: Integritätsprüfung (Blake3-Hashing, Vollständigkeitsprüfung)

**Public API**: Siehe `deckdrop-core/src/lib.rs`

### deckdrop-ui

**Zweck**: Iced-basierte Benutzeroberfläche

**Module**:
- `main.rs`: Einstiegspunkt, initialisiert Network-Bridge und Iced-App
- `app.rs`: Haupt-App-Struktur (State, Message, Update, View)
- `network_bridge.rs`: Bridge zwischen Tokio Network-Thread und Iced UI-Thread

**Architektur-Pattern**: Elm-ähnlich (State → Message → Update → View)

### deckdrop-network

**Zweck**: Peer-to-Peer Netzwerk-Funktionalität

**Module**:
- `discovery.rs`: Peer-Discovery (mDNS), Event-Handling, Request-Response
- `games.rs`: Game-Metadaten und Chunk-Transfer (Codecs, Messages)
- `peer.rs`: Peer-Informationen

**Netzwerk-Stack**: libp2p (mDNS, Request-Response, TCP)

## Event-Flow

### Network-Events → UI

```
Network-Thread (Tokio)
    │
    │ DiscoveryEvent
    ▼
mpsc::Receiver (in network_bridge.rs)
    │
    │ (global OnceLock)
    ▼
Iced UI-Thread
    │
    │ Message::Tick (alle 100ms)
    ▼
app.rs::update() → handle_network_event()
    │
    ▼
State-Update
```

### UI-Actions → Network

```
Iced UI-Thread
    │
    │ Message::DownloadGame
    ▼
app.rs::update()
    │
    │ DownloadRequest
    ▼
tokio::sync::mpsc::UnboundedSender (in network_bridge.rs)
    │
    ▼
Network-Thread (Tokio)
    │
    ▼
discovery.rs::start_discovery() → Request-Response
```

## Datenfluss

### Download-Prozess

1. **UI**: User klickt "Get this game"
2. **UI → Network**: `DownloadRequest::RequestGameMetadata`
3. **Network**: Sendet `GameMetadataRequest` an Peer
4. **Network → UI**: `DiscoveryEvent::GameMetadataReceived`
5. **UI → Core**: `start_game_download()` erstellt Manifest
6. **UI → Network**: `DownloadRequest::RequestChunk` für fehlende Chunks
7. **Network**: Sendet `ChunkRequest` an Peer
8. **Network → UI**: `DiscoveryEvent::ChunkReceived`
9. **UI → Core**: Chunk wird gespeichert, Manifest aktualisiert
10. **UI**: Progress-Bar wird aktualisiert (via `Message::Tick`)

### Chunk-System

- **Chunk-Größe**: 100MB
- **Chunk-Format**: `{file_hash}:{chunk_index}`
- **Speicherort**: `{download_path}/temp/{game_id}/`
- **Manifest**: `{config_dir}/games/{game_id}/manifest.json`
- **Hash-Algorithmus**: Blake3 (exklusiv)

## Threading-Modell

- **Iced UI-Thread**: Haupt-Thread, synchron
- **Tokio Network-Thread**: Separater Thread, asynchron
- **Kommunikation**: 
  - Network → UI: `mpsc::Receiver<DiscoveryEvent>` (polling via `Message::Tick`)
  - UI → Network: `tokio::sync::mpsc::UnboundedSender<DownloadRequest>`

## Migration von GTK zu Iced

Die Anwendung wurde von GTK4 zu Iced 0.13 migriert. Die alte GTK-Implementierung ist in `deckdrop-ui/src/main_gtk_backup.rs` gespeichert.

**Hauptänderungen**:
- GTK Widgets → Iced Widgets
- GTK Signals → Iced Messages
- GTK Callbacks → Iced Update-Funktion
- GTK Builder → Iced View-Funktion

Siehe `MIGRATION_GTK_TO_ICED.md` für Details.
