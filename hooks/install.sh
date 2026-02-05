#!/bin/bash

# Install Git hooks for the project

HOOKS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GIT_HOOKS_DIR="$(git rev-parse --git-dir)/hooks"

echo "Installing Git hooks..."

# Install pre-push hook
if [ -f "$HOOKS_DIR/pre-push" ]; then
    cp "$HOOKS_DIR/pre-push" "$GIT_HOOKS_DIR/pre-push"
    chmod +x "$GIT_HOOKS_DIR/pre-push"
    echo "✓ Installed pre-push hook"
else
    echo "✗ pre-push hook not found in $HOOKS_DIR"
    exit 1
fi

echo ""
echo "Git hooks installed successfully!"
echo ""
echo "The pre-push hook will now enforce:"
echo "  - CHANGELOG.md updates on every push"
echo "  - Version bumps in Cargo.toml on every push"
echo ""
