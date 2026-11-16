#!/bin/sh
# Build the online page into web/www/. Run it in the root of the repository.
#   web/www/pkg/          one thread, stable Rust: for a browser that is not cross-origin isolated.
#   web/www/pkg-threads/  rayon on Web Workers (wasm-bindgen-rayon). It needs nightly Rust with rust-src (the
#                         standard library is built again with atomics). The maximum memory is 4 GiB.
#   web/www/regions.json  the regions of download.geofabrik.de (id, name, parent, file), from its index-v1-nogeom.json.
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
# Geofabrik sends no CORS headers, so the page cannot read its index: a copy goes with the page
if [ ! -s web/www/regions.json ] || [ -n "$REGIONS" ]; then
  curl -sSf https://download.geofabrik.de/index-v1-nogeom.json | python3 -c '
import json, sys
out = [[f["properties"]["id"], f["properties"]["name"], f["properties"].get("parent", ""), f["properties"]["urls"]["pbf"]]
       for f in json.load(sys.stdin)["features"] if "pbf" in f["properties"].get("urls", {})]
json.dump(out, sys.stdout, separators=(",", ":"))' > web/www/regions.json
fi
