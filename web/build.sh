#!/bin/sh
# Build the browser page into web/www/. Run it in the root of the repository.
#   web/www/pkg/          one thread, stable Rust: for a browser that is not cross-origin isolated.
#   web/www/pkg-threads/  rayon on Web Workers (wasm-bindgen-rayon). It needs nightly Rust with rust-src (the
#                         standard library is built again with atomics). The maximum memory is 4 GiB.
#   web/www/regions.json  (in git) the regions of download.geofabrik.de (id, name, parent, file), and web/www/regions/ the
#                         outline of each region (web/regions.py); REGIONS=1 refreshes them.
# It needs wasm-bindgen 0.2.129 (the version in web/Cargo.toml).
set -e
NIGHTLY=${NIGHTLY:-nightly-2026-10-15}
cargo build --release --target wasm32-unknown-unknown --manifest-path web/Cargo.toml --target-dir target/web
wasm-bindgen --target web --out-dir web/www/pkg target/web/wasm32-unknown-unknown/release/pbfcut_web.wasm
# the linker flags of the threads example of wasm-bindgen
RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals -C link-arg=--shared-memory
  -C link-arg=--max-memory=4294967296 -C link-arg=--import-memory -C link-arg=--export=__wasm_init_tls
  -C link-arg=--export=__tls_size -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base' \
  cargo +$NIGHTLY build --release --target wasm32-unknown-unknown --manifest-path web/Cargo.toml --features threads \
  --target-dir target/web-threads -Z build-std=panic_abort,std
wasm-bindgen --target web --out-dir web/www/pkg-threads target/web-threads/wasm32-unknown-unknown/release/pbfcut_web.wasm
# Geofabrik sends no CORS headers, so the page cannot read its index: a copy is in git (web/www/regions.json, and the
# outline of each region in web/www/regions/). REGIONS=1 gets a new copy (the Pages workflow); if Geofabrik does not
# answer, the copy in git stays.
if [ -n "$REGIONS" ]; then
  if curl -sSf --retry 3 --retry-delay 5 https://download.geofabrik.de/index-v1.json -o target/regions-index.json; then
    python3 web/regions.py target/regions-index.json
  else
    echo "build.sh: Geofabrik did not answer; the copy of the regions in git stays" >&2
  fi
fi
