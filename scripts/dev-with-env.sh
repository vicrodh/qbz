#!/usr/bin/env bash
# Development script that loads .env variables before building

set -a
if [ -f .env ]; then
  source .env
fi
set +a

npm run tauri dev
