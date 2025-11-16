# 📦 DeckDrop

**DeckDrop** is a peer-to-peer tool for the Steam Deck (and Linux) that lets you share open-source and free games across devices in the same local network (LAN). It's optimized for the Steam Deck but fully compatible with any Linux system.

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## 🚀 Features

- 📁 **Share unpacked games** - No zip files needed, share games directly from their directories
- 🔍 **Automatic peer discovery** - Uses libp2p and mDNS for zero-configuration LAN discovery
- 🧩 **Chunk-based file transfer** - Torrent-style chunking (100MB chunks) for efficient large file transfers
- 🧠 **Early seeding** - Upload what you've downloaded while still downloading
- 🖥 **Modern Iced GUI** - Clean, minimalist interface built with Iced 0.13
- 🎮 **Steam Deck optimized** - Perfect for adding games as Non-Steam shortcuts
- 🔒 **Integrity checking** - Blake3 hashing ensures file integrity
- ⏸️ **Download management** - Pause, resume, and cancel downloads with progress tracking
- 🌐 **Fully decentralized** - No central server required, pure P2P

## 🛠 Tech Stack

- **Rust** - Core language
- **Iced 0.13** - Cross-platform GUI framework
- **libp2p** - Peer-to-peer networking stack
- **Tokio** - Asynchronous runtime
- **Blake3** - Fast cryptographic hashing
- **TOML** - Configuration file format

## 📋 Requirements

- Rust 1.70 or higher
- Linux (tested on Steam Deck, Ubuntu, Arch Linux)
- Local network (LAN) connection

## 🔧 Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/deckdrop.git
cd deckdrop

# Build the project
cargo build --release

# Run the application
cargo run --release --bin deckdrop -p deckdrop-ui
```

### Building for Steam Deck

```bash
# Build release binary
cargo build --release --bin deckdrop -p deckdrop-ui

# Binary will be at: target/release/deckdrop
```

## 📖 Usage

### First Launch

1. **Accept License** - Read and accept the terms of service
2. **Configure Settings** - Set your player name and download directory
3. **Add Games** - Click "+ Spiel hinzufügen" to add games from your system

### Sharing Games

1. Navigate to **"Meine Spiele"** tab
2. Click **"+ Spiel hinzufügen"**
3. Select the game directory
4. Fill in game metadata (name, version, start file, etc.)
5. Your game is now available to other peers on the network

### Downloading Games

1. Navigate to **"Spiele im Netzwerk"** tab
2. Browse available games from peers
3. Click **"Get this game"** to start download
4. Monitor progress with the progress bar
5. Use **Pause/Resume/Cancel** buttons as needed

### Managing Downloads

- **Progress Tracking**: Real-time progress bar shows download percentage
- **Pause**: Temporarily pause a download
- **Resume**: Continue a paused download
- **Cancel**: Abort download and clean up temporary files

## 🏗️ Project Structure

```
deckdrop/
├── deckdrop-core/      # Core logic (no UI dependencies)
│   ├── config.rs       # Configuration management
│   ├── game.rs         # Game metadata and management
│   ├── synch.rs        # Download synchronization
│   └── gamechecker.rs  # Integrity checking
├── deckdrop-ui/        # Iced-based user interface
│   ├── main.rs         # Application entry point
│   ├── app.rs          # Main app structure
│   └── network_bridge.rs # Tokio ↔ Iced integration
└── deckdrop-network/   # P2P networking
    └── network/        # Discovery, games, peers
```

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run core tests only
cargo test --lib -p deckdrop-core

# Run network tests only
cargo test -p deckdrop-network

# Run with output
cargo test -- --nocapture
```

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

### Development Setup

```bash
# Install dependencies
cargo build

# Run in development mode
cargo run --bin deckdrop -p deckdrop-ui

# Run with random ID (for testing purposes)
cargo run -- --random-id

# Run tests
cargo test
```

## 📝 How It Works

### Peer Discovery

DeckDrop uses **mDNS** (multicast DNS) to automatically discover peers on your local network. No configuration needed - just start the app and peers appear automatically.

### Chunk System

Games are split into **100MB chunks** for efficient transfer:

- Large files are automatically chunked
- Chunks can be downloaded from multiple peers
- Parallel downloads (default: 3 chunks per peer)
- Integrity verified using Blake3 hashing

### Download Process

1. **Metadata Request**: Request game metadata (`deckdrop.toml` and `deckdrop_chunks.toml`)
2. **Manifest Creation**: Create download manifest tracking all chunks
3. **Chunk Requests**: Request missing chunks from available peers
4. **Chunk Storage**: Store chunks temporarily in `{download_path}/temp/{game_id}/`
5. **File Reconstruction**: Reconstruct files from chunks when complete
6. **Integrity Check**: Verify file integrity using Blake3 hashes
7. **Cleanup**: Remove temporary chunks after successful download

## 🔐 Security & Privacy

- **LAN-only**: No internet connection required, works entirely on local network
- **Cryptographic hashing**: Blake3 ensures file integrity
- **No central server**: Fully decentralized, no data leaves your network
- **Persistent peer IDs**: Ed25519 keys for secure peer identification

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Built with [Iced](https://github.com/iced-rs/iced) - A cross-platform GUI library
- Powered by [libp2p](https://github.com/libp2p/rust-libp2p) - The modular networking stack
- Uses [Blake3](https://github.com/BLAKE3-team/BLAKE3) - Fast cryptographic hashing

## 🐛 Known Issues

- Large downloads may take time depending on network speed
- Requires all peers to be on the same local network
- First-time setup requires manual game addition

## 🔮 Roadmap

- [ ] Resume interrupted downloads
- [ ] Game launching from UI
- [ ] Game update notifications
- [ ] Bandwidth throttling
- [ ] DHT support for larger networks
- [ ] Steam Deck Decky plugin integration

## 📞 Support

For issues, questions, or contributions, please open an issue on GitHub.

---

**Made with ❤️ for the Steam Deck community**
