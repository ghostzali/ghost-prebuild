#!/usr/bin/env bash
# Phase 4: Full crate rename — xai-grok-* → ghost-*, xai-* → ghost-*
# Run from the repository root: bash scripts/rename-crates.sh
set -euo pipefail

echo "=== Phase 4: Crate Rename ==="
echo "Renaming 75+ crates from xai-grok-* / xai-* to ghost-*"
echo

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── Step 1: Rename crate directories ───────────────────────────────────
echo "Step 1: Renaming crate directories..."
# xai-grok-* → ghost-*
for dir in crates/codegen/xai-grok-*; do
    [ -d "$dir" ] || continue
    newname="${dir/xai-grok-/ghost-}"
    if [ "$dir" != "$newname" ]; then
        mv "$dir" "$newname"
        echo "  $dir → $newname"
    fi
done

# xai-* shared crates → keep (xai-tool-types, xai-chat-state, xai-hunk-tracker, etc.)
# These are shared infrastructure, not grok-specific.

# ── Step 2: Update per-crate Cargo.toml files ──────────────────────────
echo
echo "Step 2: Updating per-crate Cargo.toml..."
for toml in crates/codegen/ghost-*/Cargo.toml; do
    [ -f "$toml" ] || continue
    # Package name
    sed -i '' 's/name = "xai-grok-/name = "ghost-/g' "$toml" 2>/dev/null || sed -i 's/name = "xai-grok-/name = "ghost-/g' "$toml"
    # Path references to sibling crates
    sed -i '' 's|path = "../xai-grok-|path = "../ghost-|g' "$toml" 2>/dev/null || sed -i 's|path = "../xai-grok-|path = "../ghost-|g' "$toml"
done

# ── Step 3: Update root Cargo.toml ─────────────────────────────────────
echo
echo "Step 3: Updating root Cargo.toml..."
# Workspace members
sed -i '' 's|"crates/codegen/xai-grok-|"crates/codegen/ghost-|g' Cargo.toml 2>/dev/null || sed -i 's|"crates/codegen/xai-grok-|"crates/codegen/ghost-|g' Cargo.toml
# Dependency paths
sed -i '' 's|path = "crates/codegen/xai-grok-|path = "crates/codegen/ghost-|g' Cargo.toml 2>/dev/null || sed -i 's|path = "crates/codegen/xai-grok-|path = "crates/codegen/ghost-|g' Cargo.toml

# ── Step 4: Global import rename ───────────────────────────────────────
echo
echo "Step 4: Updating Rust imports..."
# xai_grok_ prefix in use statements
find crates/ -name '*.rs' -exec sed -i '' 's/use xai_grok_/use ghost_/g' {} \; 2>/dev/null
find crates/ -name '*.rs' -exec sed -i 's/use xai_grok_/use ghost_/g' {} \; 2>/dev/null || true

# xai_grok:: namespace references
find crates/ -name '*.rs' -exec sed -i '' 's/xai_grok::/ghost::/g' {} \; 2>/dev/null
find crates/ -name '*.rs' -exec sed -i 's/xai_grok::/ghost::/g' {} \; 2>/dev/null || true

# References to sibling crates in Cargo.toml deps (workspace = true)
find crates/ -name 'Cargo.toml' -exec sed -i '' 's/xai-grok-\([a-z]\)/ghost-\1/g' {} \; 2>/dev/null || find crates/ -name 'Cargo.toml' -exec sed -i 's/xai-grok-\([a-z]\)/ghost-\1/g' {} \; 2>/dev/null || true

# ── Step 5: Binary rename ──────────────────────────────────────────────
echo
echo "Step 5: Updating binary artifact name..."
# In the pager-bin crate
if [ -f crates/codegen/ghost-pager-bin/Cargo.toml ]; then
    sed -i '' 's/name = "xai-grok-pager"/name = "ghost"/g' crates/codegen/ghost-pager-bin/Cargo.toml 2>/dev/null || sed -i 's/name = "xai-grok-pager"/name = "ghost"/g' crates/codegen/ghost-pager-bin/Cargo.toml
fi

echo
echo "=== Crate rename complete ==="
echo "Run 'cargo check' to verify the build."
echo "Expected: 75+ crates renamed, all imports updated."
