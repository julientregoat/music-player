- error handling
  - crates: thiserror, anyhow?
- ensure simd for track parsing?
- FIX GTK+ MEMORY LEAK
  - only happens once an audio stream is started, data is fine till then
  - LINUX: once audio is played, memory starts to steadily climb up. pausing/playing
  doesn't seem to make a difference, if anything playing more may cause more problems
  - NB - MAC: looks like the memory shoots up once the track is paused, and drops back down after being played. 
  - TODO verify if this is a linux only problem?


## linux -> windows x86_64-pc-windows-gnu cross compilation

deps
- mingw64-pkg-config
- mingw64-gcc
- mingw64-openssl
- mingw64-gtk3

# configuration implementation

- `open_or_create` - librarian looks for an existing database in the local data directory (e.g. `$HOME/Library/Application Support` on mac, `$HOME/.local/share` on linux)
  - if it doesn't exist, create it along with all other intermediary folders needed
  - migrations should be located in same data dir as db
- user configuration is stored in the db. if this is a fresh DB, it should
use system based defaults
