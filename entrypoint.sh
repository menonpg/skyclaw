#!/bin/bash
set -e

# ── Ray's Startup Script ─────────────────────────────────────
# Clone workspace from GitHub on container start

WORKSPACE_DIR="$HOME/.skyclaw/workspace"

echo "Ray starting up..."

# Configure git identity
git config --global user.name "menonx"
git config --global user.email "menonx@themenonlab.com"

# Clone or pull workspace
if [ -n "$GITHUB_TOKEN" ]; then
    if [ -d "$WORKSPACE_DIR/.git" ]; then
        echo "Pulling latest workspace..."
        cd "$WORKSPACE_DIR"
        git pull origin main || true
    else
        echo "Cloning workspace..."
        mkdir -p "$WORKSPACE_DIR"
        git clone "https://menonx:${GITHUB_TOKEN}@github.com/menonpg/ray-workspace.git" "$WORKSPACE_DIR" || {
            echo "Clone failed, initializing fresh workspace..."
            cd "$WORKSPACE_DIR"
            git init
            git remote add origin "https://menonx:${GITHUB_TOKEN}@github.com/menonpg/ray-workspace.git"
        }
    fi
    echo "Workspace ready at $WORKSPACE_DIR"
else
    echo "Warning: GITHUB_TOKEN not set, workspace sync disabled"
fi

# Start SkyClaw
exec ./skyclaw start
