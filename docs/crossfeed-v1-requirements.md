# Crossfeed v1 Requirements (Formal Draft)

## 1) Product Scope
- Crossfeed is a Burp-like intercepting proxy suite written in Rust.
- v1 focuses on proxy, timeline, replay, fuzzer-style fuzzing, and codec utilities.
- Future features (scanner, plugins, reverse proxy, HTTP/3) are explicitly out of v1 scope and must not be blocked by v1 design.

## 2) Platform & Packaging
- Supported platforms: Linux (primary), Windows, macOS.
- GUI and TUI are separate binaries sharing the same project format and storage.

## 3) Project & Storage
- Folder-based project format containing:
  - SQLite database (default storage backend).
  - Project configuration (filters, scope, layout, theme, fonts).
- Data retention is indefinite per project.
- Target throughput: 250k requests/day.
- SQLite performance configuration required: WAL mode, batched writes, and indexed queries.

## 4) Proxy & Capture
- Intercepting proxy supports HTTP/1.1 and HTTP/2.
- Supports upstream proxy configuration including SOCKS.
- Scope rules support wildcard and regex for host/path; configurable to host-only and/or path-only matching.
- Filtering rules apply pre-save (capture filter) and post-save (history filter).

## 5) Timeline & Analysis
- All proxied and tool-generated traffic is stored in the timeline.
- Timeline supports sort, filter, and search by common dimensions (time, host, path, method, status, tags, scope, tool).
- Response bodies are stored and retrievable with configurable size limits.

## 6) Replay
- Primary v1 feature.
- Supports editing, resending, and comparing requests/responses.
- Supports named collections/groups; default group name is `METHOD path`.
- Diff/compare view included in v1.
- Integrates with codec for quick transforms.

## 7) Fuzzer Module
- Supports multiple payload positions.
- Fuzzes all specified positions in a single run.
- Payload processing rules (prefix/suffix/encoding) supported in v1.
- Response analysis supports grep/extract.
- Uses codec for transformations.

## 8) Codec Utilities
- Base encodings: URL, Base64, HTML, Hex, gzip/deflate (as appropriate).
- Hashing: MD5, SHA-1, SHA-256, SHA-512.
- Available standalone and embedded in fuzzer/replay workflows.

## 9) UI/UX
- Native Rust GUI (no WebView) and TUI, built on shared core logic.
- Modular, dockable panels with persistent layouts per project.
- Theme system: fully customizable colors.
- Presets: dark (default), light, gruvbox.
- Fonts: user-configurable family and size.

## 10) TLS / Certificate Handling
- Custom CA generation supported.
- Installation guidance provided inside UI and in README (no automatic OS installation in v1).

## 11) Engineering Constraints
- Strict TDD: tests written before implementation.
- Modular crates so features are reusable outside the Crossfeed binary.
- v1 decisions must not prevent future scanners, plugin system, reverse proxy, or HTTP/3.

## 12) Network Core (crossfeed-net)
- Low-level HTTP/1.1 parsing and serialization.
- HTTP/2 framing and stream primitives.
- TLS MITM certificate generation + caching.
- Client-side SOCKS support primitives.
- No Crossfeed-specific logic; usable outside the suite.

## 13) Web Library (Separate Crate)
- Separate crate used by Crossfeed.
- Async client APIs:
  - `request()`
  - `request_batch()` (stream responses as they complete)
  - `request_custom_batch()` (per-request config)
  - `download()` (to local file)
- Configurable: rate limits, proxy, headers, debug logging.
- Must be benchmarked faster than httpx, especially for batch.
- Python bindings in a separate module (PyO3 + async).
- Client implementation is custom (no reqwest/hyper).

---

# v1 Acceptance Checklist

## Proxy & Capture
- [ ] Intercept HTTP/1.1 and HTTP/2.
- [ ] SOCKS upstream proxy supported.
- [ ] Scope filtering supports wildcard + regex for host/path.
- [ ] Pre-save and post-save filtering work as expected.

## Storage & History
- [ ] Project is folder-based with SQLite DB + config.
- [ ] WAL + batching + indexes enabled.
- [ ] Handles 250k requests/day without UI lockups.
- [ ] History sort/filter/search works on key fields.

## Replay
- [ ] Group collections with default `METHOD path`.
- [ ] Edit/resend supports concurrent tabs.
- [ ] Diff/compare view available.
- [ ] Codec integration works.

## Fuzzer
- [ ] Multiple payload positions supported.
- [ ] Fuzzes all positions.
- [ ] Payload processing rules apply.
- [ ] Grep/extract response analysis.

## Codec
- [ ] Base encodings/decodings + hashing.
- [ ] Available standalone and integrated.

## UI/UX
- [ ] Native Rust GUI + TUI.
- [ ] Dockable layout, save/restore.
- [ ] Dark default, light + gruvbox presets.
- [ ] Custom fonts and sizes.

## TLS
- [ ] CA generation.
- [ ] Installation guidance in UI + README.

## Network Core (crossfeed-net)
- [ ] Server-side HTTP/1.1 + HTTP/2 primitives.
- [ ] TLS MITM certificate generation + caching.
- [ ] Client-side SOCKS primitives.

## Web Library
- [ ] Separate crate + python bindings.
- [ ] All required API methods.
- [ ] Configurable client.
- [ ] Benchmarked against httpx (batch faster).
