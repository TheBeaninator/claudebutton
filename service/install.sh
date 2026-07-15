#!/usr/bin/env bash
# Build (release) and install ClaudeButton as a root systemd service.
# Supersedes the old python jx05-remote service if present.
#
# Run it EITHER as your normal user (it will sudo for the install steps) OR with
# sudo (it drops back to your user for the cargo build). cargo must NOT run as
# root -- root's PATH picks up an older system cargo that can't read the v4
# lockfile.
set -e
HERE="$(cd "$(dirname "$0")/.." && pwd)"

BUILD_USER="${SUDO_USER:-$(id -un)}"
SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"

echo "building release binary as user '$BUILD_USER'..."
if [ "$(id -u)" -eq 0 ] && [ -n "$SUDO_USER" ]; then
  # invoked via sudo: build as the real user through a login shell (gets ~/.cargo on PATH)
  sudo -u "$SUDO_USER" -i bash -c "cd '$HERE' && cargo build --release"
else
  ( cd "$HERE" && cargo build --release )
fi

# retire the python-era service if it's installed
if systemctl list-unit-files 2>/dev/null | grep -q '^jx05-remote.service'; then
  echo "disabling old jx05-remote.service (superseded)"
  $SUDO systemctl disable --now jx05-remote || true
  $SUDO rm -f /etc/systemd/system/jx05-remote.service
fi

echo "installing service"
$SUDO cp "$HERE/service/claudebutton.service" /etc/systemd/system/claudebutton.service
$SUDO systemctl daemon-reload
$SUDO systemctl enable --now claudebutton
sleep 1
$SUDO systemctl status claudebutton --no-pager -l | head -15 || true
echo
echo "DONE. Focus any window and press a button on a known device."
echo "  logs:    journalctl -u claudebutton -f"
echo "  stop:    sudo systemctl stop claudebutton"
