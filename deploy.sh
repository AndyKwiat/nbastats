#!/usr/bin/env bash
# Build a static musl binary + static files + seed DB, then deploy to radify.
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release -j2 --target x86_64-unknown-linux-musl
rm -rf dist && mkdir -p dist/bin dist/seed
cp target/x86_64-unknown-linux-musl/release/app dist/bin/app
cp -r static dist/static
# Ship the current database as a seed; the server copies it into $DATA_DIR on first start only.
if [ -f data/nba.db ]; then sqlite3 data/nba.db "VACUUM INTO 'dist/seed/nba.db'"; fi
${RADIFY:-radify} deploy nbastats dist
