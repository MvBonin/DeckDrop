#!/bin/bash
# Build Script für DeckDrop AppImage
# Basiert auf .github/workflows/build.yml

set -e  # Exit on error

echo "=== DeckDrop AppImage Build Script ==="

# Prüfe ob wir im richtigen Verzeichnis sind
if [ ! -f "Cargo.toml" ]; then
    echo "Fehler: Cargo.toml nicht gefunden. Bitte im Projekt-Root ausführen."
    exit 1
fi

# Schritt 1: Prüfe Dependencies
echo ""
echo "=== Prüfe Dependencies ==="
MISSING_DEPS=()

if ! command -v cargo &> /dev/null; then
    echo "Warnung: cargo nicht gefunden. Stelle sicher, dass Rust installiert ist."
fi

# Erkenne Package Manager
if command -v pacman &> /dev/null; then
    PACKAGE_MANAGER="pacman"
    # Arch Linux Paketnamen
    FUSE_PKG="fuse2"
    PATCHELF_PKG="patchelf"
    WGET_PKG="wget"
    
    # Prüfe ob Pakete installiert sind
    if ! pacman -Q fuse2 &> /dev/null; then
        MISSING_DEPS+=("fuse2")
    fi
    if ! command -v patchelf &> /dev/null; then
        MISSING_DEPS+=("patchelf")
    fi
    if ! command -v wget &> /dev/null; then
        MISSING_DEPS+=("wget")
    fi
elif command -v apt-get &> /dev/null; then
    PACKAGE_MANAGER="apt"
    # Debian/Ubuntu Paketnamen
    FUSE_PKG="libfuse2"
    PATCHELF_PKG="patchelf"
    WGET_PKG="wget"
    
    # Prüfe ob Pakete installiert sind
    if ! dpkg -l | grep -q libfuse2; then
        MISSING_DEPS+=("libfuse2")
    fi
    if ! command -v patchelf &> /dev/null; then
        MISSING_DEPS+=("patchelf")
    fi
    if ! command -v wget &> /dev/null; then
        MISSING_DEPS+=("wget")
    fi
else
    echo "Warnung: Kein unterstützter Package Manager gefunden (pacman oder apt-get)"
    echo "Bitte installiere manuell: fuse2/libfuse2, patchelf, wget"
    PACKAGE_MANAGER="unknown"
fi

if [ ${#MISSING_DEPS[@]} -gt 0 ]; then
    echo "Fehlende Dependencies: ${MISSING_DEPS[*]}"
    
    if [ "$PACKAGE_MANAGER" = "pacman" ]; then
        echo "Installiere mit: sudo pacman -S ${MISSING_DEPS[*]}"
        read -p "Möchtest du die Dependencies jetzt installieren? (j/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[JjYy]$ ]]; then
            sudo pacman -S --noconfirm "${MISSING_DEPS[@]}"
        else
            echo "Bitte Dependencies manuell installieren und Script erneut ausführen."
            exit 1
        fi
    elif [ "$PACKAGE_MANAGER" = "apt" ]; then
        echo "Installiere mit: sudo apt-get install -y ${MISSING_DEPS[*]}"
        read -p "Möchtest du die Dependencies jetzt installieren? (j/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[JjYy]$ ]]; then
            sudo apt-get update
            sudo apt-get install -y "${MISSING_DEPS[@]}"
        else
            echo "Bitte Dependencies manuell installieren und Script erneut ausführen."
            exit 1
        fi
    else
        echo "Bitte Dependencies manuell installieren und Script erneut ausführen."
        exit 1
    fi
fi

# Schritt 2: Build Binary
echo ""
echo "=== Baue Binary ==="
cargo build --release --package deckdrop-ui

if [ ! -f "target/release/deckdrop" ]; then
    echo "Fehler: Binary wurde nicht erstellt!"
    exit 1
fi

echo "Binary erfolgreich erstellt: target/release/deckdrop"

# Schritt 3: Bereite AppDir vor
echo ""
echo "=== Bereite AppDir vor ==="

# Lösche altes AppDir falls vorhanden
if [ -d "AppDir" ]; then
    echo "Lösche altes AppDir..."
    rm -rf AppDir
fi

mkdir -p AppDir/usr/bin
mkdir -p AppDir/usr/share/applications
mkdir -p AppDir/usr/share/icons/hicolor/256x256/apps

# Kopiere Binary
cp target/release/deckdrop AppDir/usr/bin/deckdrop
echo "Binary kopiert nach AppDir/usr/bin/deckdrop"

# Erstelle Desktop-File
echo "Erstelle Desktop-File..."
cat > AppDir/usr/share/applications/DeckDrop.desktop << 'EOF'
[Desktop Entry]
Type=Application
Name=DeckDrop
Comment=Local P2P game sharing for Steam Deck
Exec=deckdrop
Icon=deckdrop
Terminal=false
Categories=Game;Utility;
StartupNotify=true
EOF

# Kopiere Desktop-File auch ins AppDir-Root
cp AppDir/usr/share/applications/DeckDrop.desktop AppDir/DeckDrop.desktop
cp AppDir/usr/share/applications/DeckDrop.desktop AppDir/deckdrop.desktop
echo "Desktop-File erstellt"

# Kopiere Icon falls vorhanden
if [ -f "assets/deckdrop.png" ]; then
    echo "Kopiere Icon..."
    cp assets/deckdrop.png AppDir/deckdrop.png
    cp assets/deckdrop.png AppDir/usr/share/icons/hicolor/256x256/apps/deckdrop.png
    echo "Icon kopiert"
else
    echo "Warnung: assets/deckdrop.png nicht gefunden"
    if command -v convert &> /dev/null; then
        echo "Erstelle Platzhalter-Icon mit ImageMagick..."
        convert -size 256x256 xc:blue -pointsize 72 -fill white -gravity center -annotate +0+0 "DD" AppDir/deckdrop.png
        cp AppDir/deckdrop.png AppDir/usr/share/icons/hicolor/256x256/apps/deckdrop.png
    else
        echo "Hinweis: ImageMagick nicht verfügbar, kein Platzhalter-Icon erstellt"
    fi
fi

# Erstelle AppRun-Script
echo "Erstelle AppRun-Script..."
cat > AppDir/AppRun << 'EOF'
#!/bin/bash
SELF="$(readlink -f "${0}")"
HERE="$(dirname "${SELF}")"
exec "${HERE}/usr/bin/deckdrop" "$@"
EOF
chmod +x AppDir/AppRun
echo "AppRun-Script erstellt"

# Verifiziere AppDir-Struktur
echo ""
echo "=== Verifiziere AppDir-Struktur ==="
ls -la AppDir/usr/share/applications/
ls -la AppDir/

# Schritt 4: Lade appimagetool herunter
echo ""
echo "=== Lade appimagetool herunter ==="

APPIMAGETOOL="appimagetool-x86_64.AppImage"

if [ ! -f "$APPIMAGETOOL" ]; then
    echo "Lade appimagetool herunter..."
    wget https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$APPIMAGETOOL"
    echo "appimagetool heruntergeladen"
else
    echo "appimagetool bereits vorhanden"
fi

# Schritt 5: Baue AppImage
echo ""
echo "=== Baue AppImage ==="

APPIMAGE_NAME="DeckDrop-x86_64.AppImage"

# Lösche altes AppImage falls vorhanden
if [ -f "$APPIMAGE_NAME" ]; then
    echo "Lösche altes AppImage..."
    rm -f "$APPIMAGE_NAME"
fi

./"$APPIMAGETOOL" AppDir "$APPIMAGE_NAME"

if [ ! -f "$APPIMAGE_NAME" ]; then
    echo "Fehler: AppImage wurde nicht erstellt!"
    exit 1
fi

# Mache AppImage ausführbar
chmod +x "$APPIMAGE_NAME"

echo ""
echo "=== Erfolg! ==="
echo "AppImage erstellt: $APPIMAGE_NAME"
echo "Größe: $(du -h "$APPIMAGE_NAME" | cut -f1)"
echo ""
echo "Du kannst das AppImage jetzt testen mit:"
echo "  ./$APPIMAGE_NAME"

