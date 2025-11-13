# Spiel-Synchronisation (Download) Protokoll

## Übersicht

Dieses Dokument beschreibt den Ablauf, wie ein Spiel von einem Peer im Netzwerk heruntergeladen wird.

## Ablauf beim Empfangen eines Spiels

### Phase 1: Initiale Anfrage

1. **Benutzer klickt "Get this game"** bei einem Spiel in der "Spiele im Netzwerk" Liste
2. **Anfrage an Peer senden**: 
   - Request: `GameMetadataRequest { game_id: String }`
   - Peer antwortet mit: `GameMetadataResponse { deckdrop_toml: String, deckdrop_chunks_toml: String }`

### Phase 2: Lokale Struktur anlegen

3. **Lokale Verzeichnisstruktur erstellen**:
   ```
   config/games/{game_id}/
   ├── manifest.json          # Download-Status und Chunk-Informationen
   └── (später) game/         # Das eigentliche Spielverzeichnis
   ```

4. **Manifest initialisieren**:
   - Lade `deckdrop_chunks.toml` und parse sie
   - Erstelle `manifest.json` mit:
     ```json
     {
       "game_id": "...",
       "game_path": "~/Games/{game_name}",
       "chunks": {
         "file_path": {
           "chunks": ["hash1", "hash2", ...],
           "status": "pending|downloading|complete",
           "downloaded_chunks": ["hash1", "hash2"]
         }
       },
       "overall_status": "pending|downloading|complete"
     }
     ```

### Phase 3: Chunk-Vergleich und Download

5. **Ermittle fehlende Chunks**:
   - Für jede Datei in `deckdrop_chunks.toml`:
     - Prüfe welche Chunks bereits lokal vorhanden sind
     - Identifiziere fehlende Chunks

6. **Peer-Map abfragen**:
   - Für jeden fehlenden Chunk:
     - Frage die globale Peer-Map: "Wer hat diesen Chunk?"
     - Chunks werden nach Hash identifiziert (unabhängig von der Datei)

7. **Chunk-Anfragen senden**:
   - Starte Request-Response-Protokoll zu den jeweiligen Peers:
     - Request: `ChunkRequest { chunk_hash: String }`
     - Response: `ChunkResponse { chunk_data: Vec<u8> }`

8. **Chunks speichern**:
   - Speichere empfangene Chunks in einem temporären Verzeichnis
   - Aktualisiere `manifest.json` mit dem Download-Status

### Phase 4: Datei-Rekonstruktion

9. **Dateien aus Chunks rekonstruieren**:
   - Wenn alle Chunks einer Datei vorhanden sind:
     - Kombiniere Chunks in der richtigen Reihenfolge
     - Validiere SHA-256 Hash des rekonstruierten Chunks
     - Schreibe Datei ins Zielverzeichnis

10. **Spielverzeichnis finalisieren**:
    - Wenn alle Dateien rekonstruiert sind:
      - Kopiere `deckdrop.toml` und `deckdrop_chunks.toml` ins Spielverzeichnis
      - Füge Spiel zur lokalen Bibliothek hinzu
      - Aktualisiere `manifest.json`: `overall_status = "complete"`

## Protokoll-Details

### GameMetadataRequest/Response

**Request:**
```rust
struct GameMetadataRequest {
    game_id: String,
}
```

**Response:**
```rust
struct GameMetadataResponse {
    deckdrop_toml: String,
    deckdrop_chunks_toml: String,
}
```

### ChunkRequest/Response

**Request:**
```rust
struct ChunkRequest {
    chunk_hash: String,  // SHA-256 Hash im Format "sha256:XYZ"
}
```

**Response:**
```rust
struct ChunkResponse {
    chunk_data: Vec<u8>,  // Die Chunk-Daten (max 10MB)
}
```

## Manifest-Struktur

```json
{
  "game_id": "abc123...",
  "game_name": "SuperTux",
  "game_path": "~/Games/SuperTux",
  "chunks": {
    "bin/supertux2": {
      "chunk_hashes": ["sha256:hash1", "sha256:hash2"],
      "status": "complete",
      "downloaded_chunks": ["sha256:hash1", "sha256:hash2"],
      "file_size": 12345678
    },
    "assets/level1.map": {
      "chunk_hashes": ["sha256:hash3"],
      "status": "downloading",
      "downloaded_chunks": ["sha256:hash3"],
      "file_size": 54321
    }
  },
  "overall_status": "downloading",
  "progress": {
    "total_chunks": 10,
    "downloaded_chunks": 7,
    "percentage": 70.0
  }
}
```

## Fehlerbehandlung

- **Chunk nicht verfügbar**: Versuche anderen Peer
- **Hash-Validierung fehlgeschlagen**: Lade Chunk erneut
- **Peer nicht erreichbar**: Markiere Chunk als "pending" und versuche später erneut
- **Unvollständiger Download**: Setze Status zurück auf "pending" und starte erneut

## Optimierungen

- **Parallele Downloads**: Lade mehrere Chunks gleichzeitig von verschiedenen Peers
- **Chunk-Caching**: Speichere Chunks zentral, um Duplikate zu vermeiden
- **Resume-Funktion**: Setze unterbrochene Downloads fort
- **Priorisierung**: Lade wichtige Dateien (z.B. Start-Datei) zuerst

