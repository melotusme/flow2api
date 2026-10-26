# Desktop media simulation

Created and exercised 2026-10-27. Local fixture derived from the archived media storage contract; no provider-owned Postman mock was searched or pulled.

## Purpose

Exercise the actual Rust media downloader and archive writer over HTTP. Routes and payloads live in `postman/mocks/flow-media/default.js`; Google login, captcha and inference are outside this fixture.

## Execution

```bash
rtk proxy postman simulation run postman/simulations/desktop-media/desktop-media.sim.yaml --port 4950 --output ndjson
# Another terminal; use the actual listening event's address + /media
rtk proxy env FLOW2API_MEDIA_SIM_URL=http://127.0.0.1:4950/media cargo test --manifest-path desktop/Cargo.toml -p flow2api-core --locked media_transport_simulation -- --ignored
# Stop simulation: Ctrl+C in its terminal
```

## Assumptions

- The local test uses an isolated temporary SQLite and media directory; it does not use Google credentials. The PNG is a small valid binary fixture, not a captured generation result. Passing this test cannot establish real Flow compatibility. (observed 2026-10-27)
- This machine's inherited USTC Cargo registry returns 404. Verification used a dedicated `CARGO_HOME` cache and invoked Cargo from `/tmp` against the absolute manifest path to avoid inheriting the ancestor `.cargo/config.toml`. The global configuration was not changed. (observed 2026-10-27)

## Observations

<!-- begin:observations -->
2026-10-27: the `media` member served all three routes to the Rust test: HTTP 503, HTTP 200 HTML, and HTTP 200 PNG. The first two left no media record; the PNG was saved with a matching SHA256 filename and JSON metadata. The explicit Cargo rerun also passed. No member remained silent.
<!-- end:observations -->
