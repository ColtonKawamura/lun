# Installation

## Prerequisites

- macOS (tested on 12+)
- Rust 1.70+ (or use `make`)
- SQLite 3+ (bundled)
- Tailscale (optional, for remote access)

## Build from Source

### Development
```sh
git clone https://github.com/ColtonKawamura/lun.git
cd lun
cargo build
./target/debug/lun
```

### Release
```sh
make
./target/release/lun
```

## Install

### Copy to PATH
```sh
cp target/release/lun /usr/local/bin/
lun --version
```

### Homebrew (PR needed)
```sh
brew tap ColtonKawamura/lun
brew install lun
```

## Initialize

```sh
lun init    # Creates .lun/lun.db
```

## Verify

```sh
lun status
lun serve   # Start web UI
```

## Configuration

### Server Bind Address
```sh
lun serve 0.0.0.0:9120
```

### Tailscale
```sh
lun serve 100.71.78.78:9120
```

### Database Location
Default: `.lun/lun.db`

Override with `LUN_DB` env var:
```sh
LUN_DB=/path/to/lun.db lun status
```

## Uninstall

```sh
rm -f /usr/local/bin/lun
```

## Troubleshooting

### Build fails
```sh
rustup update
cargo clean
make
```

### Server won't start
```sh
# Check if port is in use
lsof -i :9120

# Kill existing server
pkill -f "lun serve"
```

### Database corrupted
```sh
sqlite3 .lun/lun.db "PRAGMA integrity_check;"
```
