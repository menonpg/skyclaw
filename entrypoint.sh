#!/bin/bash
set -e

# ── Ray's Startup Script ─────────────────────────────────────
APP_DIR="/app"
WORKSPACE_DIR="$HOME/.skyclaw/workspace"

echo "Ray starting up..."

git config --global user.name "menonx"
git config --global user.email "menonx@themenonlab.com"

if [ -n "$GITHUB_TOKEN" ]; then
    mkdir -p "$WORKSPACE_DIR"
    if [ -d "$WORKSPACE_DIR/.git" ]; then
        echo "Volume workspace exists — pulling latest identity/memory..."
        (cd "$WORKSPACE_DIR" && git pull origin main) || echo "Pull failed, continuing with existing workspace..."
    else
        echo "Fresh volume — cloning workspace..."
        git clone "https://menonx:${GITHUB_TOKEN}@github.com/menonpg/ray-workspace.git" "$WORKSPACE_DIR" 2>/dev/null || {
            echo "Clone failed, initializing empty workspace..."
            (cd "$WORKSPACE_DIR" && git init) || true
        }
    fi
    echo "Workspace ready at $WORKSPACE_DIR"
else
    echo "Warning: GITHUB_TOKEN not set, workspace sync disabled"
    mkdir -p "$WORKSPACE_DIR"
fi

# Start from workspace so relative paths (git clone, file writes) land
# in the persistent volume by default, not the ephemeral /app/ directory.
cd "$WORKSPACE_DIR"
exec "$APP_DIR/skyclaw" start
