#!/bin/bash

set -e  # Exit on any error

echo "🚀 Building DeckDrop Flatpak..."

# Check prerequisites
echo "🔍 Checking prerequisites..."
if ! command -v flatpak &> /dev/null; then
    echo "❌ flatpak is not installed. Please install it first."
    exit 1
fi

if ! command -v flatpak-builder &> /dev/null; then
    echo "❌ flatpak-builder is not installed. Please install it first."
    exit 1
fi

# Check if GNOME runtime is installed
if ! flatpak list | grep -q "org.gnome.Platform"; then
    echo "⚠️  GNOME Platform runtime not found. Installing..."
    flatpak install flathub org.gnome.Platform//48 org.gnome.Sdk//48
fi

# Build the DEB package first
echo "📦 Building DEB package..."
bun run tauri build

# Check if DEB was created
DEB_PATH="src-tauri/target/release/bundle/deb/deckdrop_0.1.0_amd64.deb"
if [ ! -f "$DEB_PATH" ]; then
    echo "❌ DEB package not found at $DEB_PATH!"
    echo "Available files in bundle/deb/:"
    ls -la src-tauri/target/release/bundle/deb/ 2>/dev/null || echo "No bundle/deb directory found"
    exit 1
fi

echo "✅ DEB package created successfully: $(ls -lh $DEB_PATH)"

# Clean previous build
echo "🧹 Cleaning previous build..."
rm -rf flatpak-repo flatpak

# Build the Flatpak
echo "📦 Building Flatpak..."
flatpak-builder --force-clean --user --disable-cache --repo flatpak-repo flatpak flatpak-builder.yaml

if [ $? -eq 0 ]; then
    echo "✅ Flatpak repository built successfully!"
    
    # Create the .flatpak bundle file
    echo "📦 Creating .flatpak bundle file..."
    flatpak build-bundle flatpak-repo deckdrop.flatpak com.github.mvbonin.deckdrop
    
    if [ $? -eq 0 ]; then
        echo "✅ Flatpak bundle created: $(ls -lh deckdrop.flatpak)"
    else
        echo "❌ Failed to create .flatpak bundle!"
        exit 1
    fi
    
    echo "🎮 Die Anwendung wird dann im Steam Deck Desktop Mode verfügbar sein!"
    echo ""
    echo "Um die App zu installieren:"
    echo "flatpak install deckdrop.flatpak"
    echo ""
    echo "Um die App zu starten:"
    echo "flatpak run com.github.mvbonin.deckdrop"
    echo ""
    echo "Um die App zu aktualisieren:"
    echo "flatpak -y --user update com.github.mvbonin.deckdrop"
    
    # Cleanup problematic directories to prevent file system watcher issues
    echo ""
    echo "🧹 Cleaning up problematic directories for development..."
    echo "   Removing symbolic links that cause file system watcher loops..."
    rm -rf flatpak/var 2>/dev/null || true
    rm -rf .flatpak-builder 2>/dev/null || true
    echo "   ✅ Cleanup completed! You can now continue development with 'bun run tauri dev'"
else
    echo "❌ Flatpak build failed!"
    exit 1
fi 