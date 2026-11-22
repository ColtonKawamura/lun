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
lun finds its board the way git finds a repo: `.lun/lun.db` in the **current
directory**, otherwise in the nearest **parent** directory that has one. The
search stops at `$HOME` (a `~/.lun` is never used), there is no override
variable, and `lun root` prints the board in use. A command that had to walk
up says `lun: using board at <dir>` on stderr. Outside any board, lun fails
with `not-initialized: no lun board found ...` — that means "wrong
directory", not "initialize here". `lun init` never walks up: it always
creates the board in the current directory (and warns if that nests inside
another board). `lun serve` never creates a board.

### Multiple boards
A board is just a directory that ran `lun init`, so separate boards (say work
and personal) are separate directories, each with its own `lun serve` on its
own port. Which board a command uses depends only on where you run it.

```sh
mkdir ~/personal-board && cd ~/personal-board && lun init
cd ~/personal-board && lun serve 100.71.78.78:9121    # work board stays on :9120
```

Each board's web page shows its directory next to the logo (`~/repos/`,
`~/personal-board/`) so the two are easy to tell apart.

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
