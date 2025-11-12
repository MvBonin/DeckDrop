# DeckDrop

# Alle Tests im Network-Modul

cargo test -p deckdrop-network

# Nur einen spezifischen Test

cargo test -p deckdrop-network test_channel_overflow_handling

# Tests mit Ausgabe

cargo test -p deckdrop-network -- --nocapture

# Terminal 1

cargo run --bin deckdrop-gtk

# Terminal 2 (mit --random-id für unterschiedliche Peer-ID)

cargo run --bin deckdrop-gtk -- --random-id
