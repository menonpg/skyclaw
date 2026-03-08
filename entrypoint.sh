#!/bin/bash
set -e

# ── Ray's Startup Script ─────────────────────────────────────
# Clone workspace from GitHub on container start

APP_DIR="/app"
WORKSPACE_DIR="$HOME/.skyclaw/workspace"

echo "Ray starting up..."

# Configure git identity
git config --global user.name "menonx"
git config --global user.email "menonx@themenonlab.com"

# Clone or pull workspace (don't fail if repo not accessible yet)
if [ -n "$GITHUB_TOKEN" ]; then
    mkdir -p "$WORKSPACE_DIR"
    if [ -d "$WORKSPACE_DIR/.git" ]; then
        echo "Pulling latest workspace..."
        (cd "$WORKSPACE_DIR" && git pull origin main) || echo "Pull failed, continuing..."
    else
        echo "Cloning workspace..."
        git clone "https://menonx:${GITHUB_TOKEN}@github.com/menonpg/ray-workspace.git" "$WORKSPACE_DIR" 2>/dev/null || {
            echo "Clone failed (repo may not be accessible yet), initializing empty workspace..."
            (cd "$WORKSPACE_DIR" && git init) || true
        }
    fi
    echo "Workspace ready at $WORKSPACE_DIR"
else
    echo "Warning: GITHUB_TOKEN not set, workspace sync disabled"
    mkdir -p "$WORKSPACE_DIR"
fi

# Start SkyClaw from app directory
cd "$APP_DIR"
exec ./skyclaw start
