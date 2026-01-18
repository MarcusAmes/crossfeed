# Crossfeed v1 Master Plan

## Workflow Rules
- Use strict TDD: write tests before implementation for each feature.
- Work milestone by milestone; only one active at a time.
- After each milestone:
  - You validate behavior.
  - I create a git commit (only after you confirm).
- No milestone starts until previous is accepted and committed.
- Any scope changes require updating this plan before proceeding.

---

# v1 Requirements (Reference)

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

# Milestones

## Milestone 0: Repo Bootstrap & Conventions
Status: [ ] Completed
**Goal:** Establish workspace structure, shared crates, and testing scaffolding.
**Includes:**
- Workspace layout for core, gui, tui, web-lib, and shared crates.
- Project format spec draft (folder structure + config file format).
- Testing conventions (naming, structure, harness).
**Acceptance Criteria:**
- Workspace builds and tests run with placeholder tests.
- Project format doc stub exists.

---

## Milestone 1: Project Format + Storage Schema
Status: [ ] Completed
**Goal:** Define folder-based project format and SQLite schema.
**Includes:**
- Project folder layout (`db.sqlite`, `project.json`, etc.).
- SQLite tables for requests, responses, metadata, tags, scope rules.
- Indexes and WAL configuration defaults.
- Migration strategy (even if v1 only).
**Acceptance Criteria:**
- Schema created with tests for inserts/queries.
- Benchmark seed script or load test harness (optional).

---

## Milestone 2: Network Core (crossfeed-net)
Status: [ ] Completed
**Goal:** Low-level HTTP/TLS foundation for proxy and clients.
**Includes:**
- HTTP/1.1 parsing and serialization primitives.
- HTTP/2 framing and stream handling primitives.
- TLS MITM certificate generation + caching.
- Client-side SOCKS support primitives.
- No Crossfeed-specific logic.
**Acceptance Criteria:**
- Core parsing and TLS tests pass.
- Clear public APIs for proxy and web client usage.

---

## Milestone 3: Proxy Core (HTTP/1.1 + HTTP/2)
Status: [ ] Completed
**Goal:** Intercepting proxy core with SOCKS upstream.
**Includes:**
- HTTP/1.1 + HTTP/2 support.
- SOCKS upstream configuration.
- TLS interception & CA generation.
- Scope matching + pre-save filtering.
**Acceptance Criteria:**
- Proxy handles basic HTTP and HTTPS flows.
- Scope filters verified by tests.

---

## Milestone 4: Web Library (Rust Only)
Status: [ ] Completed
**Goal:** Build the custom async web client crate used by Crossfeed.
**Includes:**
- `request`, `request_batch`, `request_custom_batch`, `download`.
- Configurable rate limits, proxy, headers, debug logging.
- Benchmark harness (httpx comparison planned).
**Acceptance Criteria:**
- API coverage tests pass.
- Benchmark harness runs.

---

## Milestone 5: Timeline Service
Status: [ ] Completed
**Goal:** Unified timeline capture, query, sort, filter.
**Includes:**
- Timeline write pipeline from proxy + tools.
- Query API supporting filters (host, path, method, status, tags, tool).
- Pagination and sorting.
- Configurable body size limits.
**Acceptance Criteria:**
- Tests for high-volume inserts + filter queries.
- Demonstrated performance constraints.

---

## Milestone 6: Codec Core
Status: [ ] Completed
**Goal:** Shared codec utilities.
**Includes:**
- URL/Base64/HTML/Hex/gzip/deflate encode/decode.
- Hashing: MD5/SHA-1/SHA-256/SHA-512.
**Acceptance Criteria:**
- Test coverage for all transforms.

---

## Milestone 7: Replay Core
Status: [ ] Completed
**Goal:** Primary v1 feature.
**Includes:**
- Request editor, resend, response view.
- Groups/collections with default `METHOD path`.
- Diff/compare view.
- Integration with codec.
**Acceptance Criteria:**
- Tests for resend, grouping, diff logic.

---

## Milestone 8: Fuzzer Core
Status: [ ] Completed
**Goal:** Fuzzing with response analysis.
**Includes:**
- Multiple payload positions.
- Fuzz all positions.
- Payload processing rules (prefix/suffix/encode).
- Grep/extract response analysis.
- Codec integration.
**Acceptance Criteria:**
- Tests for payload generation + analysis.

---

## Milestone 9: GUI Shell (Native Rust)
Status: [ ] Completed
**Goal:** Functional GUI with dockable layout.
**Includes:**
- Native Rust GUI framework selection.
- Dockable panels (proxy, timeline, replay, fuzzer, codec).
- Layout persistence per project.
- Theming (dark default, light, gruvbox) + fonts.
- TLS guidance UI screens.
**Acceptance Criteria:**
- Layout save/restore works.
- Theme and font changes apply.

---

## Milestone 10: TUI Shell
Status: [ ] Completed
**Goal:** Functional TUI using shared core.
**Includes:**
- Basic navigation for proxy, timeline, replay, fuzzer, codec.
- Uses same project format + storage.
**Acceptance Criteria:**
- Feature parity for core workflows (view, repeat, fuzz).

---

## Milestone 11: Python Bindings for Web Library
Status: [ ] Completed
**Goal:** Provide PyO3 async bindings.
**Includes:**
- Separate bindings module.
- Async interop (tokio + pyo3-asyncio).
- API parity with Rust client.
**Acceptance Criteria:**
- Python-side tests pass.
- Example usage works in bbot context.

---

# Out-of-Scope for v1
- Reverse proxy mode
- Passive/active scanner
- Plugin system
- HTTP/3
