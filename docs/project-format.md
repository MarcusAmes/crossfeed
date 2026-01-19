# Crossfeed Project Format (Draft)

## Overview
Crossfeed projects are stored as folders that contain configuration and data files.

## Folder Layout (Draft)
- `project.toml`: project configuration (scope, filters, UI layout, theme, fonts)
- `db.sqlite`: SQLite database containing timeline, replay, and fuzzer data
- `exports/`: user-triggered exports (optional)
- `logs/`: diagnostic logs (optional)

## Project Configuration (Draft)
```toml
[timeline.body_limits_mb]
request_max_mb = 40
response_max_mb = 40
```

## Database Schema (Draft)
Tables and indexes are defined in the storage crate schema catalog. SQL definitions
will be finalized before Milestone 1 completes, aligned with timeline/replay/fuzzer naming.

Core tables in v1:
- timeline: `timeline_sources`, `timeline_requests`, `timeline_responses`
- replay: `replay_collections`, `replay_requests`, `replay_versions`, `replay_executions`
- tags: `tags`, `timeline_request_tags`
- scope: `scope_rules`

## Notes
- Schema and file contents will be refined during Milestone 1.
- This document will be updated as the project format stabilizes.
