# 📦 DeckDrop

**DeckDrop** is a peer-to-peer tool for the Steam Deck (and Linux) that lets you share open-source and free games across devices in the same local network (LAN). It's optimized for the Steam Deck but fully compatible with any Linux system.

## 🚀 Features

- 📁 Share unpacked games (no zip files needed)
- 🔍 Automatic peer discovery using libp2p and mDNS
- 🧩 Chunk-based file transfer (Torrent-style but LAN-only)
- 🧠 Early seeding: upload what you've downloaded
- 🖥 Tauri-based GUI for ease of use
- 🎮 Add games as Non-Steam shortcuts

## 🛠 Tech Stack

- Rust + Tokio for async networking
- libp2p for peer discovery and chunk transfer
- Tauri + Svelte for cross-platform GUI
- Bun for fast JavaScript/TypeScript development
- toml for per-game metadata (`game.toml`)

## 🚀 Quick Start

### Prerequisites

1. **Install Bun** (if not already installed):

   ```bash
   curl -fsSL https://bun.sh/install | bash
   ```

2. **Install Rust** (if not already installed):

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source ~/.cargo/env
   ```

3. **Install Tauri CLI**:
   ```bash
   cargo install tauri-cli
   ```

### Development

1. **Clone and install dependencies**:

   ```bash
   git clone <repository-url>
   cd DeckDrop
   bun install
   ```

2. **Start development server**:

   ```bash
   bun run tauri dev
   ```

   This will:

   - Start the SvelteKit development server
   - Compile and run the Tauri backend
   - Open the app window with hot reload

3. **For development with multiple instances** (to test peer discovery):

   ```bash
   # Terminal 1
   bun run tauri dev

   # Terminal 2 (different port)
   bun run tauri dev --port 1421
   ```

### Building

1. **Build for development**:

   ```bash
   bun run build
   ```

2. **Build for production**:

   ```bash
   bun run tauri build
   ```

   This creates a DEB package in `src-tauri/target/release/bundle/deb/`

3. **Build Flatpak for Steam Deck**:

   ```bash
   ./build-flatpak.sh
   ```

   This creates a Flatpak package that can be installed on Steam Deck and other Linux systems.

4. **Install Flatpak on Steam Deck**:

   ```bash
   # Copy the flatpak-repo to your Steam Deck
   # Then install:
   flatpak install --user flatpak-repo com.github.mvbonin.deckdrop
   
   # Run the app:
   flatpak run com.github.mvbonin.deckdrop
   ```

### Available Scripts

```bash
# Development
bun run dev          # Start development server
bun run tauri dev    # Start Tauri development
bun run build        # Build frontend only

# Building
bun run tauri build  # Build production app
bun run tauri build --debug  # Build debug version

# Utilities
bun run check        # Type check
bun run lint         # Lint code
bun run format       # Format code
```

## 💡 Game Metadata (game.toml)

```toml
name = "SuperTux"
exec = "SuperTux2/supertux2"
args = "--fullscreen"
version = "0.6.3"
tags = ["platformer", "opensource"]
hash = "sha256:abcd1234..."
```

## 🔧 Troubleshooting

### Common Issues

1. **Port already in use**:

   ```bash
   bun run tauri dev --port 1421
   ```

2. **Rust compilation errors**:

   ```bash
   cd src-tauri
   cargo clean
   cargo build
   ```

3. **Bun installation issues**:
   ```bash
   # Reinstall Bun
   rm -rf ~/.bun
   curl -fsSL https://bun.sh/install | bash
   ```

### Development Tips

- Use `bun run tauri dev` for the best development experience
- The app automatically discovers other instances on the same network
- Check the console for peer discovery logs
- Use multiple terminal windows to test peer discovery locally

## 📦 License

MIT or GPLv3 — to be decided based on target community
