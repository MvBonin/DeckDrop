# Download-Funktionalität: Implementierungsplan

## Status-Übersicht

### ✅ Bereits implementiert:
- [x] Network-Protokolle (`GameMetadataRequest/Response`, `ChunkRequest/Response`)
- [x] Server-Seite: Callbacks für GameMetadataLoader und ChunkLoader
- [x] Event-System: `GameMetadataReceived`, `ChunkReceived`, `ChunkRequestFailed`
- [x] `DownloadManifest` Struktur mit Save/Load
- [x] Chunk-Speicherung und -Validierung (`save_chunk`, `load_chunk`)
- [x] Datei-Rekonstruktion (`reconstruct_file`)
- [x] `start_game_download` Funktion (erstellt Manifest)
- [x] Event-Handler für `GameMetadataReceived` im GTK-Thread

### ❌ Noch zu implementieren:

## Phase 1: Download-Button Handler (Priorität: HOCH)

### 1.1 Request-System für Downloads
**Datei**: `deckdrop-gtk/src/main.rs`

**Aufgabe**: 
- Channel zwischen GTK-Thread und Tokio-Thread für Download-Requests
- Button-Handler sendet `GameMetadataRequest` an ersten verfügbaren Peer

**Implementierung**:
```rust
// Channel für Download-Requests (vom GTK-Thread zum Tokio-Thread)
let (download_request_tx, mut download_request_rx) = tokio::sync::mpsc::unbounded_channel::<DownloadRequest>();

enum DownloadRequest {
    RequestGameMetadata { peer_id: String, game_id: String },
    RequestChunk { peer_id: String, chunk_hash: String },
}
```

**Button-Handler**:
```rust
download_button.connect_clicked(move |_| {
    // Wähle ersten verfügbaren Peer
    if let Some(peer_id) = peer_ids.first() {
        // Sende Request über Channel
        download_request_tx.send(DownloadRequest::RequestGameMetadata {
            peer_id: peer_id.clone(),
            game_id: game_id_clone.clone(),
        }).unwrap();
    }
});
```

### 1.2 Request-Handler im Tokio-Thread
**Datei**: `deckdrop-network/src/network/discovery.rs`

**Aufgabe**:
- Empfange Download-Requests vom GTK-Thread
- Sende tatsächliche libp2p Requests an Peers
- Tracke Request-IDs mit chunk_hashes für korrekte Zuordnung

**Implementierung**:
- HashMap: `request_id -> (chunk_hash, game_id)` für Chunk-Requests
- HashMap: `request_id -> game_id` für Metadata-Requests
- Handler im Tokio-Event-Loop liest `download_request_rx`

## Phase 2: Chunk-Download-Logik (Priorität: HOCH)

### 2.1 Chunk-Request-Tracking
**Problem**: `ChunkReceived` Event enthält keine `chunk_hash`, nur `chunk_data`

**Lösung**:
- Erweitere `DiscoveryEvent::ChunkReceived` um `request_id`
- Oder: Tracke `request_id -> chunk_hash` Mapping
- Oder: Validiere Hash des empfangenen Chunks und finde passenden Eintrag im Manifest

**Beste Lösung**: Request-ID Tracking
```rust
// In discovery.rs
struct PendingChunkRequest {
    chunk_hash: String,
    game_id: String,
    peer_id: String,
}

let mut pending_chunks: HashMap<RequestId, PendingChunkRequest> = HashMap::new();

// Beim Senden eines Chunk-Requests:
let request_id = swarm.behaviour_mut().chunks.send_request(&peer_id, request);
pending_chunks.insert(request_id, PendingChunkRequest {
    chunk_hash: request.chunk_hash.clone(),
    game_id: game_id.clone(),
    peer_id: peer_id.to_string(),
});

// Beim Empfang:
if let Some(pending) = pending_chunks.remove(&request_id) {
    event_tx.send(DiscoveryEvent::ChunkReceived {
        peer_id: pending.peer_id,
        chunk_hash: pending.chunk_hash,
        chunk_data: response.chunk_data,
    }).await;
}
```

### 2.2 Automatischer Chunk-Download nach Metadaten-Empfang
**Datei**: `deckdrop-gtk/src/synch.rs`

**Aufgabe**:
- Nach `GameMetadataReceived`:
  1. Lade Manifest
  2. Ermittle fehlende Chunks (`get_missing_chunks()`)
  3. Für jeden fehlenden Chunk: Sende `ChunkRequest` an verfügbaren Peer

**Implementierung**:
```rust
pub fn request_missing_chunks(
    game_id: &str,
    peer_ids: &[String],
    download_request_tx: &tokio::sync::mpsc::UnboundedSender<DownloadRequest>,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = get_manifest_path(game_id)?;
    let mut manifest = DownloadManifest::load(&manifest_path)?;
    
    let missing_chunks = manifest.get_missing_chunks();
    
    // Verteile Chunks auf verfügbare Peers (Round-Robin)
    for (i, chunk_hash) in missing_chunks.iter().enumerate() {
        let peer_id = &peer_ids[i % peer_ids.len()];
        download_request_tx.send(DownloadRequest::RequestChunk {
            peer_id: peer_id.clone(),
            chunk_hash: chunk_hash.clone(),
        })?;
    }
    
    Ok(())
}
```

### 2.3 Chunk-Empfang und -Speicherung
**Datei**: `deckdrop-gtk/src/main.rs` (Event-Handler)

**Aufgabe**:
- Empfange `ChunkReceived` Event
- Speichere Chunk mit `save_chunk()`
- Aktualisiere Manifest
- Prüfe ob Datei komplett → Rekonstruiere Datei
- Prüfe ob Spiel komplett → Finalisiere Download

**Implementierung**:
```rust
DiscoveryEvent::ChunkReceived { peer_id, chunk_hash, chunk_data } => {
    // 1. Finde game_id aus chunk_hash (durch Manifest-Suche)
    let game_id = find_game_id_for_chunk(&chunk_hash)?;
    
    // 2. Speichere Chunk
    let chunks_dir = synch::get_chunks_dir(&game_id)?;
    synch::save_chunk(&chunk_hash, &chunk_data, &chunks_dir)?;
    
    // 3. Aktualisiere Manifest
    let manifest_path = synch::get_manifest_path(&game_id)?;
    let mut manifest = synch::DownloadManifest::load(&manifest_path)?;
    manifest.mark_chunk_downloaded(&chunk_hash);
    manifest.save(&manifest_path)?;
    
    // 4. Prüfe ob Dateien komplett sind
    for (file_path, file_info) in &manifest.chunks {
        if file_info.status == synch::DownloadStatus::Complete 
            && !file_info.downloaded_chunks.is_empty() {
            // Rekonstruiere Datei
            let output_path = PathBuf::from(&manifest.game_path).join(file_path);
            synch::reconstruct_file(
                file_path,
                &file_info.chunk_hashes,
                &chunks_dir,
                &output_path,
            )?;
        }
    }
    
    // 5. Prüfe ob Spiel komplett ist
    if manifest.overall_status == synch::DownloadStatus::Complete {
        finalize_game_download(&game_id, &manifest)?;
    }
}
```

## Phase 3: Chunk-Loader Verbesserung (Priorität: MITTEL)

### 3.1 Korrekte Chunk-Extraktion
**Datei**: `deckdrop-gtk/src/main.rs` (ChunkLoader Callback)

**Problem**: Aktuell wird nur der erste Chunk einer Datei geladen, unabhängig von der Position

**Lösung**:
- Parse `deckdrop_chunks.toml` korrekt
- Finde Datei, die den angefragten Chunk enthält
- Berechne Chunk-Position in der Datei (basierend auf Chunk-Index)
- Lade nur den spezifischen Chunk (10MB Offset)

**Implementierung**:
```rust
// Finde Chunk in chunks.toml
for entry in chunks_data {
    if let Some(chunks) = entry.get("chunks").and_then(|c| c.as_array()) {
        for (chunk_index, chunk) in chunks.iter().enumerate() {
            if let Some(chunk_str) = chunk.as_str() {
                let chunk_hash_clean = chunk_str.strip_prefix("sha256:").unwrap_or(chunk_str);
                if chunk_hash_clean == hash_name {
                    // Chunk gefunden - lade spezifischen Chunk
                    let full_path = game_path.join(entry["path"].as_str().unwrap());
                    let mut file = std::fs::File::open(&full_path)?;
                    file.seek(SeekFrom::Start((chunk_index * CHUNK_SIZE) as u64))?;
                    let mut buffer = vec![0u8; CHUNK_SIZE];
                    let bytes_read = file.read(&mut buffer)?;
                    buffer.truncate(bytes_read);
                    
                    // Validiere Hash
                    let mut hasher = Sha256::new();
                    hasher.update(&buffer);
                    let computed_hash = hex::encode(hasher.finalize());
                    if computed_hash == hash_name {
                        return Some(buffer);
                    }
                }
            }
        }
    }
}
```

## Phase 4: Fehlerbehandlung und Retry-Logik (Priorität: MITTEL)

### 4.1 Retry bei fehlgeschlagenen Chunk-Requests
**Datei**: `deckdrop-gtk/src/main.rs`

**Aufgabe**:
- Bei `ChunkRequestFailed`: Versuche anderen Peer
- Max. 3 Versuche pro Chunk
- Tracke fehlgeschlagene Versuche im Manifest

**Implementierung**:
```rust
DiscoveryEvent::ChunkRequestFailed { peer_id, chunk_hash, error } => {
    // Finde game_id
    let game_id = find_game_id_for_chunk(&chunk_hash)?;
    
    // Lade Manifest
    let manifest_path = synch::get_manifest_path(&game_id)?;
    let manifest = synch::DownloadManifest::load(&manifest_path)?;
    
    // Finde andere verfügbare Peers für dieses Spiel
    let available_peers = find_peers_with_game(&game_id, &network_games);
    
    // Versuche anderen Peer
    if let Some(next_peer) = available_peers.iter()
        .find(|p| *p != &peer_id) {
        download_request_tx.send(DownloadRequest::RequestChunk {
            peer_id: next_peer.clone(),
            chunk_hash: chunk_hash.clone(),
        })?;
    } else {
        // Keine weiteren Peers verfügbar - markiere als Fehler
        eprintln!("Keine weiteren Peers für Chunk {} verfügbar", chunk_hash);
    }
}
```

### 4.2 Hash-Validierung bei Chunk-Empfang
**Bereits implementiert** in `save_chunk()`, aber sollte auch bei `ChunkReceived` geprüft werden

## Phase 5: UI-Verbesserungen (Priorität: NIEDRIG)

### 5.1 Download-Fortschrittsanzeige
**Aufgabe**:
- Zeige Download-Status für jedes Spiel in der Netzwerk-Liste
- Progress-Bar für laufende Downloads
- Button-Text ändern: "Downloading..." / "Downloaded" / "Get this game"

**Implementierung**:
- Erweitere `update_network_games_list` um Download-Status-Check
- Lade Manifest für jedes Spiel und prüfe Status
- Zeige Progress-Bar oder Status-Label

### 5.2 Download-Verwaltung
**Aufgabe**:
- Liste aller laufenden Downloads
- Möglichkeit Downloads zu pausieren/fortzusetzen
- Möglichkeit Downloads abzubrechen

## Phase 6: Finalisierung (Priorität: HOCH)

### 6.1 Spiel-Finalisierung
**Datei**: `deckdrop-gtk/src/synch.rs`

**Aufgabe**:
- Wenn alle Dateien rekonstruiert sind:
  1. Kopiere `deckdrop.toml` und `deckdrop_chunks.toml` ins Spielverzeichnis
  2. Füge Spiel zur lokalen Bibliothek hinzu (`config.add_game_path()`)
  3. Lösche temporäre Chunk-Dateien (optional)
  4. Aktualisiere UI (Spiel erscheint in lokaler Liste)

**Implementierung**:
```rust
pub fn finalize_game_download(
    game_id: &str,
    manifest: &DownloadManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let game_path = PathBuf::from(&manifest.game_path);
    
    // 1. Kopiere Metadaten
    let manifest_dir = get_manifest_path(game_id)?.parent().unwrap();
    let deckdrop_toml_src = manifest_dir.join("deckdrop.toml");
    let deckdrop_chunks_toml_src = manifest_dir.join("deckdrop_chunks.toml");
    
    fs::copy(&deckdrop_toml_src, game_path.join("deckdrop.toml"))?;
    fs::copy(&deckdrop_chunks_toml_src, game_path.join("deckdrop_chunks.toml"))?;
    
    // 2. Füge zur Bibliothek hinzu
    let mut config = crate::config::Config::load();
    config.add_game_path(&game_path)?;
    
    // 3. Optional: Lösche Chunks
    // let chunks_dir = get_chunks_dir(game_id)?;
    // fs::remove_dir_all(&chunks_dir)?;
    
    Ok(())
}
```

## Tests

### Unit Tests

#### 1. `synch.rs` Tests
**Datei**: `deckdrop-gtk/src/synch.rs` (am Ende)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_download_manifest_from_chunks_toml() {
        let chunks_toml = r#"
[[file]]
path = "test.bin"
chunks = ["sha256:abc123", "sha256:def456"]
"#;
        
        let manifest = DownloadManifest::from_chunks_toml(
            "test_game".to_string(),
            "Test Game".to_string(),
            "/tmp/test".to_string(),
            chunks_toml,
        ).unwrap();
        
        assert_eq!(manifest.game_id, "test_game");
        assert_eq!(manifest.progress.total_chunks, 2);
        assert!(manifest.chunks.contains_key("test.bin"));
    }
    
    #[test]
    fn test_get_missing_chunks() {
        // Erstelle Manifest mit einigen bereits heruntergeladenen Chunks
        // Prüfe ob get_missing_chunks() korrekt funktioniert
    }
    
    #[test]
    fn test_chunk_hash_validation() {
        // Teste save_chunk() mit korrektem und falschem Hash
    }
    
    #[test]
    fn test_file_reconstruction() {
        // Erstelle Test-Chunks
        // Rekonstruiere Datei
        // Validiere Ergebnis
    }
}
```

#### 2. `game.rs` Tests (Chunk-Generierung)
**Datei**: `deckdrop-gtk/src/game.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generate_chunks_toml() {
        // Erstelle Test-Verzeichnis mit Dateien
        // Generiere chunks.toml
        // Validiere Hash-Berechnung
    }
    
    #[test]
    fn test_chunk_hash_in_deckdrop_toml() {
        // Prüfe ob Hash der chunks.toml korrekt in deckdrop.toml gespeichert wird
    }
}
```

### Integration Tests

#### 3. Zwei-Peer Download-Test
**Datei**: `deckdrop-network/src/network/games.rs` oder neue Datei

**Aufgabe**:
- Erstelle zwei Peers
- Peer 1 hat ein Spiel mit Chunks
- Peer 2 lädt das Spiel von Peer 1 herunter
- Validiere dass alle Chunks korrekt übertragen werden
- Validiere dass Dateien korrekt rekonstruiert werden

**Hinweis**: Dieser Test ist komplex und könnte ähnliche Probleme wie der vorherige Integration-Test haben. Eventuell besser als manueller Test.

### Manuelle Tests

#### 4. End-to-End Download-Test
**Aufgabe**:
1. Starte zwei Instanzen der Anwendung (mit `--random-id`)
2. Füge in Instanz 1 ein Spiel hinzu
3. Warte bis Spiel in Instanz 2 erscheint
4. Klicke "Get this game" in Instanz 2
5. Validiere:
   - Metadaten werden empfangen
   - Chunks werden heruntergeladen
   - Dateien werden rekonstruiert
   - Spiel erscheint in lokaler Bibliothek von Instanz 2

## Prioritäten

### Sofort (für funktionierenden Download):
1. ✅ Phase 1: Download-Button Handler
2. ✅ Phase 2.1: Chunk-Request-Tracking
3. ✅ Phase 2.2: Automatischer Chunk-Download
4. ✅ Phase 2.3: Chunk-Empfang und -Speicherung
5. ✅ Phase 6.1: Spiel-Finalisierung

### Danach (für Robustheit):
6. Phase 3.1: Korrekte Chunk-Extraktion
7. Phase 4: Fehlerbehandlung und Retry

### Optional (für UX):
8. Phase 5: UI-Verbesserungen

## Offene Fragen

1. **Chunk-Caching**: Sollen Chunks zentral gespeichert werden, um Duplikate zu vermeiden? (z.B. wenn mehrere Spiele denselben Chunk haben)
2. **Parallele Downloads**: Wie viele Chunks gleichzeitig von einem Peer?
3. **Resume-Funktion**: Soll ein unterbrochener Download fortgesetzt werden können?
4. **Chunk-Löschung**: Wann sollen temporäre Chunk-Dateien gelöscht werden?

