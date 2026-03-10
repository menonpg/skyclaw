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
        # Clone to temp dir first to avoid failure when lost+found exists on fresh ext4 volume
        TMPCLONE=$(mktemp -d)
        if git clone "https://x-access-token:${GITHUB_TOKEN}@github.com/menonpg/ray-workspace.git" "$TMPCLONE" 2>/dev/null; then
            cp -r "$TMPCLONE/." "$WORKSPACE_DIR/"
            rm -rf "$TMPCLONE"
            echo "Clone succeeded."
        else
            echo "Clone failed, initializing empty workspace..."
            rm -rf "$TMPCLONE"
            (cd "$WORKSPACE_DIR" && git init) || true
        fi
    fi
    echo "Workspace ready at $WORKSPACE_DIR"

    # ── Restore work repos to persistent volume ───────────────
    # These repos are cloned as subdirectories so they survive Railway restarts.
    # Clone if missing; pull if already there.
    restore_repo() {
        local name="$1"
        local dest="$WORKSPACE_DIR/$name"
        if [ -d "$dest/.git" ]; then
            echo "Repo $name exists — pulling..."
            (cd "$dest" && git pull origin main 2>/dev/null || git pull origin master 2>/dev/null) || echo "Pull failed for $name, continuing..."
        else
            echo "Cloning $name into persistent volume..."
            git clone "https://x-access-token:${GITHUB_TOKEN}@github.com/menonpg/${name}.git" "$dest" \
                || echo "Clone failed for $name, continuing..."
        fi
    }

    restore_repo "407singles"
    restore_repo "menonlab-blog"

else
    echo "Warning: GITHUB_TOKEN not set, workspace sync disabled"
    mkdir -p "$WORKSPACE_DIR"
fi

# Start from APP_DIR so skyclaw finds skyclaw.toml (copied there by Dockerfile)
cd "$APP_DIR"
exec ./skyclaw start
