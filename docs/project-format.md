# Crossfeed Project Format (Draft)

## Overview
Crossfeed projects are stored as folders that contain configuration and data files.

## Folder Layout (Draft)
- `project.toml`: project configuration (scope, filters, UI layout, theme, fonts)
- `db.sqlite`: SQLite database containing history and metadata
- `exports/`: user-triggered exports (optional)
- `logs/`: diagnostic logs (optional)

## Notes
- Schema and file contents will be defined in Milestone 1.
- This document will be updated as the project format stabilizes.
