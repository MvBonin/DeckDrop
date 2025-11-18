//! Plattformübergreifende Window-Sichtbarkeitssteuerung

/// Zeigt das Fenster an (plattformübergreifend)
pub fn show_window() {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // Verwende AppleScript, um das Fenster zu zeigen
        // Versuche mehrere Methoden für bessere Kompatibilität
        let script1 = r#"
            tell application "System Events"
                try
                    set deckdropProcess to first process whose name is "deckdrop"
                    set frontmost of deckdropProcess to true
                    set visible of deckdropProcess to true
                end try
            end tell
        "#;
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(script1)
            .output();
        
        // Alternative: Versuche die Anwendung direkt zu aktivieren
        let script2 = r#"
            tell application "deckdrop" to activate
        "#;
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(script2)
            .output();
    }
    
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        // Verwende xdotool, um das Fenster zu zeigen
        // Finde das Fenster mit dem Namen "DeckDrop"
        if let Ok(output) = Command::new("xdotool")
            .args(&["search", "--name", "DeckDrop"])
            .output()
        {
            if let Ok(window_id) = String::from_utf8(output.stdout) {
                let window_id = window_id.trim();
                if !window_id.is_empty() {
                    let _ = Command::new("xdotool")
                        .args(&["windowactivate", window_id])
                        .output();
                }
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        use winapi::um::winuser::{FindWindowW, ShowWindow, SW_SHOW, SW_RESTORE};
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        
        // Finde das Fenster mit dem Titel "DeckDrop"
        let title: Vec<u16> = OsStr::new("DeckDrop")
            .encode_wide()
            .chain(Some(0))
            .collect();
        
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_RESTORE);
                ShowWindow(hwnd, SW_SHOW);
            }
        }
    }
}

/// Versteckt das Fenster (plattformübergreifend)
pub fn hide_window() {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        // Verwende AppleScript, um das Fenster zu verstecken
        // Versuche mehrere Methoden für bessere Kompatibilität
        let script1 = r#"
            tell application "System Events"
                try
                    set deckdropProcess to first process whose name is "deckdrop"
                    set visible of deckdropProcess to false
                end try
            end tell
        "#;
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(script1)
            .output();
        
        // Alternative: Versuche die Anwendung zu minimieren
        let script2 = r#"
            tell application "System Events"
                try
                    set deckdropProcess to first process whose name is "deckdrop"
                    set minimized of windows of deckdropProcess to true
                end try
            end tell
        "#;
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(script2)
            .output();
    }
    
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        // Verwende xdotool, um das Fenster zu verstecken
        if let Ok(output) = Command::new("xdotool")
            .args(&["search", "--name", "DeckDrop"])
            .output()
        {
            if let Ok(window_id) = String::from_utf8(output.stdout) {
                let window_id = window_id.trim();
                if !window_id.is_empty() {
                    let _ = Command::new("xdotool")
                        .args(&["windowminimize", window_id])
                        .output();
                }
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        use winapi::um::winuser::{FindWindowW, ShowWindow, SW_MINIMIZE};
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        
        // Finde das Fenster mit dem Titel "DeckDrop"
        let title: Vec<u16> = OsStr::new("DeckDrop")
            .encode_wide()
            .chain(Some(0))
            .collect();
        
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_MINIMIZE);
            }
        }
    }
}

