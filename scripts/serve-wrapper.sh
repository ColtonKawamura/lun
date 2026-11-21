#!/bin/bash
# Wrapper for `lun serve` — polls for Tailscale IP before exec'ing,
# so launchd never sees a fast-exit loop when Tailscale is down.

BIND="100.71.78.78:9120"
IP="100.71.78.78"

# Wait for Tailscale interface
for i in $(seq 1 60); do
    if ifconfig utun 2>/dev/null | grep -q "$IP"; then
        break
    fi
    sleep 2
done

if ! ifconfig utun 2>/dev/null | grep -q "$IP"; then
    echo "lun serve: Tailscale IP $IP not found after 120s — exiting"
    exit 1
fi

# Resolve the lun binary — adjust to match your install path
LUN_BIN="/Users/colton/repos/lun/target/release/lun"
if [ ! -x "$LUN_BIN" ]; then
    echo "lun serve: binary not found at $LUN_BIN"
    exit 1
fi

exec "$LUN_BIN" serve "$BIND"
