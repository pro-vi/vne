#!/usr/bin/env sh
set -eu

npm run check
npm run build
npm run test
npm run test:rust
