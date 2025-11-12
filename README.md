# DeckDrop

# Alle Tests im Network-Modul

cargo test -p deckdrop-network

# Nur einen spezifischen Test

cargo test -p deckdrop-network test_channel_overflow_handling

# Tests mit Ausgabe

cargo test -p deckdrop-network -- --nocapture
