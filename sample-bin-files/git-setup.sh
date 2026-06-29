#!/bin/sh
# Per-clone setup for the Gendaldea vault.
#
# Run once on each machine after cloning. These are *git config* settings,
# which are per-clone and do NOT sync through the repo — so each machine
# (this laptop, the Mac) needs them set locally. Safe to re-run; idempotent.
#
#   ./setup.sh
#
set -e
cd "$(git rev-parse --show-toplevel)"

git config core.hooksPath .githooks   # enable the pre-commit guard (bin/vault-check.rb)
git config pull.rebase false          # pull = merge, never rebase (avoids stuck-rebase conflicts)

echo "Gendaldea vault — per-clone setup complete:"
echo "  core.hooksPath = $(git config core.hooksPath)"
echo "  pull.rebase    = $(git config pull.rebase)"
echo
echo "Also (one-time, in the Obsidian app): set obsidian-git's merge strategy to"
echo "prefer LOCAL ('ours'), so a sync never overwrites the machine you're on."
