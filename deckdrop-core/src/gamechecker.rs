use std::fs;
use std::path::{Path, PathBuf};
use blake3;
use hex;
use serde::Deserialize;

/// Berechnet den Blake3 Hash einer Datei
/// Diese Funktion kann von verschiedenen Stellen im Code verwendet werden
/// 
/// Blake3 ist sehr schnell und sicher.
pub fn calculate_file_hash(file_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(file_path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; 8192]; // 8KB Buffer für effizientes Lesen
    
    use std::io::Read;
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    
    let hash = hasher.finalize();
    Ok(hex::encode(hash.as_bytes()))
}

/// Validiert einen Blake3 Hash
pub fn verify_hash(file_path: &Path, expected_hash: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let computed_hash = calculate_file_hash(file_path)?;
    Ok(computed_hash == expected_hash)
}

/// Struktur für einen Eintrag in deckdrop_chunks.toml
#[derive(Debug, Clone, Deserialize)]
struct ChunkFileEntry {
    path: String,
    file_hash: String,
    chunk_count: i64,
    file_size: i64,
}

/// Ergebnis der Integritätsprüfung
#[derive(Debug, Clone)]
pub struct GameIntegrityResult {
    pub total_files: usize,
    pub verified_files: usize,
    pub failed_files: Vec<IntegrityError>,
    pub missing_files: Vec<String>,
}

/// Fehler bei der Integritätsprüfung
#[derive(Debug, Clone)]
pub struct IntegrityError {
    pub file_path: String,
    pub error: String,
}

/// Ergebnis des Light-Checks (schnell, prüft nur Datei-Existenz)
#[derive(Debug, Clone)]
pub struct LightCheckResult {
    pub total_files: usize,
    pub existing_files: usize,
    pub missing_files: Vec<String>,
    pub extra_files: Vec<String>, // Dateien die nicht in deckdrop_chunks.toml sind
}

/// Prüft die Integrität und Vollständigkeit eines Spiels
/// Validiert alle Dateien anhand ihrer Hashes aus deckdrop_chunks.toml
pub fn verify_game_integrity(game_path: &Path) -> Result<GameIntegrityResult, Box<dyn std::error::Error>> {
    verify_game_integrity_with_progress::<fn(usize, usize)>(game_path, None)
}

/// Prüft die Integrität und Vollständigkeit eines Spiels mit Progress-Callback
/// Der Callback wird nach jeder geprüften Datei aufgerufen: callback(current_index, total_files)
pub fn verify_game_integrity_with_progress<F>(
    game_path: &Path,
    progress_callback: Option<F>
) -> Result<GameIntegrityResult, Box<dyn std::error::Error>>
where
    F: Fn(usize, usize),
{
    let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
    
    if !chunks_toml_path.exists() {
        return Err("deckdrop_chunks.toml nicht gefunden".into());
    }
    
    let chunks_toml_content = fs::read_to_string(&chunks_toml_path)?;
    
    // Parse TOML - deckdrop_chunks.toml hat Format: [[file]]
    #[derive(Deserialize)]
    struct ChunksToml {
        file: Vec<ChunkFileEntry>,
    }
    
    let chunks_data: ChunksToml = toml::from_str(&chunks_toml_content)?;
    
    let total_files = chunks_data.file.len();
    let mut result = GameIntegrityResult {
        total_files,
        verified_files: 0,
        failed_files: Vec::new(),
        missing_files: Vec::new(),
    };
    
    for (index, entry) in chunks_data.file.iter().enumerate() {
        let file_path = game_path.join(&entry.path);
        
        if !file_path.exists() {
            result.missing_files.push(entry.path.clone());
            if let Some(ref callback) = progress_callback {
                callback(index + 1, total_files);
            }
            continue;
        }
        
        // Lese Datei und berechne Hash
        let file_data = fs::read(&file_path)?;
        
        // Prüfe Dateigröße
        if file_data.len() as i64 != entry.file_size {
            result.failed_files.push(IntegrityError {
                file_path: entry.path.clone(),
                error: format!("Dateigröße stimmt nicht: erwartet {}, erhalten {}", entry.file_size, file_data.len()),
            });
            if let Some(ref callback) = progress_callback {
                callback(index + 1, total_files);
            }
            continue;
        }
        
        // Berechne Hash mit wiederverwendbarer Funktion (Blake3)
        let computed_hash = calculate_file_hash(&file_path)?;
        
        // Prüfe Hash
        if computed_hash != entry.file_hash {
            result.failed_files.push(IntegrityError {
                file_path: entry.path.clone(),
                error: format!("Hash stimmt nicht: erwartet {}, erhalten {}", 
                    entry.file_hash, computed_hash),
            });
            if let Some(ref callback) = progress_callback {
                callback(index + 1, total_files);
            }
            continue;
        }
        
        result.verified_files += 1;
        
        // Callback nach erfolgreicher Prüfung
        if let Some(ref callback) = progress_callback {
            callback(index + 1, total_files);
        }
    }
    
    Ok(result)
}

/// Light-Check: Prüft schnell, ob Dateien hinzugekommen oder verschwunden sind
/// Validiert NICHT die Hashes (schneller)
pub fn light_check_game(game_path: &Path) -> Result<LightCheckResult, Box<dyn std::error::Error>> {
    let chunks_toml_path = game_path.join("deckdrop_chunks.toml");
    
    if !chunks_toml_path.exists() {
        return Err("deckdrop_chunks.toml nicht gefunden".into());
    }
    
    let chunks_toml_content = fs::read_to_string(&chunks_toml_path)?;
    
    // Parse TOML
    #[derive(Deserialize)]
    struct ChunksToml {
        file: Vec<ChunkFileEntry>,
    }
    
    let chunks_data: ChunksToml = toml::from_str(&chunks_toml_content)?;
    
    // Erstelle Set der erwarteten Dateien
    let expected_files: std::collections::HashSet<String> = chunks_data.file
        .iter()
        .map(|e| e.path.clone())
        .collect();
    
    // Sammle alle Dateien im Spielverzeichnis (rekursiv)
    let mut actual_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    
    fn collect_files(dir: &Path, base: &Path, files: &mut std::collections::HashSet<String>) -> Result<(), Box<dyn std::error::Error>> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                
                // Überspringe deckdrop.toml und deckdrop_chunks.toml
                if let Some(file_name) = path.file_name() {
                    let file_name_str = file_name.to_string_lossy();
                    if file_name_str == "deckdrop.toml" || file_name_str == "deckdrop_chunks.toml" {
                        continue;
                    }
                }
                
                if path.is_dir() {
                    collect_files(&path, base, files)?;
                } else if path.is_file() {
                    if let Ok(relative) = path.strip_prefix(base) {
                        let relative_str = relative.to_string_lossy().replace('\\', "/");
                        files.insert(relative_str);
                    }
                }
            }
        }
        Ok(())
    }
    
    collect_files(game_path, game_path, &mut actual_files)?;
    
    // Finde fehlende Dateien
    let missing_files: Vec<String> = expected_files
        .iter()
        .filter(|f| !actual_files.contains(*f))
        .cloned()
        .collect();
    
    // Finde zusätzliche Dateien (nicht in deckdrop_chunks.toml)
    let extra_files: Vec<String> = actual_files
        .iter()
        .filter(|f| !expected_files.contains(*f))
        .cloned()
        .collect();
    
    Ok(LightCheckResult {
        total_files: expected_files.len(),
        existing_files: expected_files.len() - missing_files.len(),
        missing_files,
        extra_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_calculate_file_hash() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, b"Test content").unwrap();
        
        let hash = calculate_file_hash(&file_path).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // Blake3 hex string length (32 bytes)
    }
    
    #[test]
    fn test_verify_hash() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, b"Test content").unwrap();
        
        let hash = calculate_file_hash(&file_path).unwrap();
        assert!(verify_hash(&file_path, &hash).unwrap());
        assert!(!verify_hash(&file_path, "wrong_hash").unwrap());
    }

    #[test]
    fn test_verify_game_integrity() {
        let temp_dir = TempDir::new().unwrap();
        let game_path = temp_dir.path();
        
        // Erstelle Test-Dateien
        let file1_data = b"Test file 1 content";
        let file2_data = b"Test file 2 content";
        
        // Berechne Hashes mit wiederverwendbarer Funktion
        let file1_path = game_path.join("file1.txt");
        fs::write(&file1_path, file1_data).unwrap();
        let hash1 = calculate_file_hash(&file1_path).unwrap();
        
        let file2_path = game_path.join("file2.txt");
        fs::write(&file2_path, file2_data).unwrap();
        let hash2 = calculate_file_hash(&file2_path).unwrap();
        
        // Erstelle deckdrop_chunks.toml
        let chunks_toml = format!(
            r#"[[file]]
path = "file1.txt"
file_hash = "{}"
chunk_count = 1
file_size = {}

[[file]]
path = "file2.txt"
file_hash = "{}"
chunk_count = 1
file_size = {}
"#,
            hash1, file1_data.len(), hash2, file2_data.len()
        );
        
        fs::write(game_path.join("deckdrop_chunks.toml"), chunks_toml).unwrap();
        
        // Prüfe Integrität
        let result = verify_game_integrity(game_path).unwrap();
        
        assert_eq!(result.total_files, 2);
        assert_eq!(result.verified_files, 2);
        assert_eq!(result.failed_files.len(), 0);
        assert_eq!(result.missing_files.len(), 0);
    }

    #[test]
    fn test_verify_game_integrity_with_corrupted_file() {
        let temp_dir = TempDir::new().unwrap();
        let game_path = temp_dir.path();
        
        // Erstelle Test-Datei
        let file_data = b"Test file content";
        let file_path = game_path.join("file.txt");
        fs::write(&file_path, file_data).unwrap();
        
        // Berechne Hash
        let hash = calculate_file_hash(&file_path).unwrap();
        
        // Erstelle deckdrop_chunks.toml
        let chunks_toml = format!(
            r#"[[file]]
path = "file.txt"
file_hash = "{}"
chunk_count = 1
file_size = {}
"#,
            hash, file_data.len()
        );
        
        fs::write(game_path.join("deckdrop_chunks.toml"), chunks_toml).unwrap();
        
        // Korrumpiere Datei
        fs::write(&file_path, b"Corrupted content").unwrap();
        
        // Prüfe Integrität
        let result = verify_game_integrity(game_path).unwrap();
        
        assert_eq!(result.total_files, 1);
        assert_eq!(result.verified_files, 0);
        assert_eq!(result.failed_files.len(), 1);
        assert!(result.failed_files[0].error.contains("Hash stimmt nicht"));
    }

    #[test]
    fn test_light_check_game() {
        let temp_dir = TempDir::new().unwrap();
        let game_path = temp_dir.path();
        
        // Erstelle Test-Dateien
        let file1_data = b"Test file 1";
        let file2_data = b"Test file 2";
        
        let file1_path = game_path.join("file1.txt");
        fs::write(&file1_path, file1_data).unwrap();
        let hash1 = calculate_file_hash(&file1_path).unwrap();
        
        let file2_path = game_path.join("file2.txt");
        fs::write(&file2_path, file2_data).unwrap();
        let hash2 = calculate_file_hash(&file2_path).unwrap();
        
        // Erstelle deckdrop_chunks.toml
        let chunks_toml = format!(
            r#"[[file]]
path = "file1.txt"
file_hash = "{}"
chunk_count = 1
file_size = {}

[[file]]
path = "file2.txt"
file_hash = "{}"
chunk_count = 1
file_size = {}
"#,
            hash1, file1_data.len(), hash2, file2_data.len()
        );
        
        fs::write(game_path.join("deckdrop_chunks.toml"), chunks_toml).unwrap();
        
        // Light-Check
        let result = light_check_game(game_path).unwrap();
        
        assert_eq!(result.total_files, 2);
        assert_eq!(result.existing_files, 2);
        assert_eq!(result.missing_files.len(), 0);
    }

    #[test]
    fn test_light_check_game_with_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let game_path = temp_dir.path();
        
        // Erstelle nur eine Datei
        let file1_data = b"Test file 1";
        let file1_path = game_path.join("file1.txt");
        fs::write(&file1_path, file1_data).unwrap();
        let hash1 = calculate_file_hash(&file1_path).unwrap();
        
        // Erstelle deckdrop_chunks.toml mit 2 Dateien (eine fehlt)
        let chunks_toml = format!(
            r#"[[file]]
path = "file1.txt"
file_hash = "{}"
chunk_count = 1
file_size = {}

[[file]]
path = "file2.txt"
file_hash = "dummy"
chunk_count = 1
file_size = 10
"#,
            hash1, file1_data.len()
        );
        
        fs::write(game_path.join("deckdrop_chunks.toml"), chunks_toml).unwrap();
        
        // Light-Check
        let result = light_check_game(game_path).unwrap();
        
        assert_eq!(result.total_files, 2);
        assert_eq!(result.existing_files, 1);
        assert_eq!(result.missing_files.len(), 1);
        assert_eq!(result.missing_files[0], "file2.txt");
    }

    #[test]
    fn test_light_check_game_with_extra_file() {
        let temp_dir = TempDir::new().unwrap();
        let game_path = temp_dir.path();
        
        // Erstelle Test-Dateien
        let file1_data = b"Test file 1";
        let file1_path = game_path.join("file1.txt");
        fs::write(&file1_path, file1_data).unwrap();
        let hash1 = calculate_file_hash(&file1_path).unwrap();
        
        // Erstelle zusätzliche Datei (nicht in deckdrop_chunks.toml)
        fs::write(game_path.join("extra.txt"), b"Extra file").unwrap();
        
        // Erstelle deckdrop_chunks.toml mit nur einer Datei
        let chunks_toml = format!(
            r#"[[file]]
path = "file1.txt"
file_hash = "{}"
chunk_count = 1
file_size = {}
"#,
            hash1, file1_data.len()
        );
        
        fs::write(game_path.join("deckdrop_chunks.toml"), chunks_toml).unwrap();
        
        // Light-Check
        let result = light_check_game(game_path).unwrap();
        
        assert_eq!(result.total_files, 1);
        assert_eq!(result.existing_files, 1);
        assert_eq!(result.missing_files.len(), 0);
        assert_eq!(result.extra_files.len(), 1);
        assert_eq!(result.extra_files[0], "extra.txt");
    }
}

