#!/usr/bin/env sh
set -eu

export CARGO_BUILD_JOBS=1
export RUST_TEST_THREADS=1
node --test scripts/test-release.mjs
npm run check
npm run build
npm run test
npm run test:rust
scripts/security-check.sh
