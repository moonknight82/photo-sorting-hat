# Photo Sorting Hat

Photo Sorting Hat is a personal project for bringing scattered photo and video folders into an organized archive. It is an offline-first desktop app and CLI built with Rust, SQLite, ExifTool, Tauri, and React, and released under the MIT License.

**Preview software:** the metadata engine is regression-tested; Lightroom Classic and Apple Photos import compatibility must still be checked with full camera originals. No personal photos are needed to run the automated tests.

## What it does

- Scan multiple local or mounted folders with a persistent index and bounded memory.
- Organize by EXIF capture date, camera, source folder, or project through saved recipes.
- Generate photo XMP sidecars and embed keywords in supported output copies.
- Preserve existing XMP/ACR editing data and merge existing and selected folder keywords.
- Keep video under a configurable separate folder using the same recipe.
- Find exact duplicates with SHA-256 and require review for differing sidecars.
- Optionally skip matched `web` and `instagram` variants when an unmarked, `Full`, `High`, `High-Res`, `Original`, or `Master` counterpart exists. Skipped sources remain untouched.
- Recognize `Events/<event name>/...` folders offline, add the event name as a keyword, and use it in output names.
- **Copy or Move:** Copy is the default. Move removes a selected source only after its full output bundle is verified. Unchanged RAW/video files use hardlinks for staging on the same filesystem when available; all publication uses atomic no-overwrite renames. Other files use verified staged copies. Skipped duplicates and shared sidecars are retained.
- Resume interrupted jobs and export JSON Lines reports with source references and hashes.
- Light, dark, and system themes. No image uploads or AI services.

## Desktop

Follow **Sources → Scan → Recipe → Review → Export**. Pick source folders and a destination, scan, choose your recipe, review all destinations, and export. For exact duplicate groups with differing sidecars, choose **Keep all separately** or explicitly choose **Use first representative** after comparing the metadata. Changing a recipe invalidates the preview until it is regenerated.

Session databases live in the app's local application-data directory. Open a previous `.sqlite` session from Preferences. A session preserves its original sources, output, and export manifest; use **New archive session** for another job.

GitHub update checks run at launch when online. Disable them in Preferences for entirely offline operation. Installation always requires a click and waits for the app to be idle. Offline failures never block the app.

## Development

Install Rust stable, Node.js 22+, pnpm, and ExifTool. Linux desktop builds also need Tauri's WebKitGTK 4.1 development prerequisites. macOS builds need Xcode Command Line Tools. See [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
cargo test
pnpm build
pnpm desktop
```

Build a distributable with its metadata runtime:

```sh
python3 scripts/bundle_metadata.py
pnpm tauri build
cargo build --release -p photo-sorting-hat
```

On the initial development workstation, `sh scripts/env.sh COMMAND ...` exposes the project-local Rust toolchain and bundled Node/pnpm without changing shell profiles. These paths are optional and ignored on other machines. `.tools` is ignored by Git.

## CLI

```sh
photo-sorting-hat --db archive.sqlite scan /Volumes/Camera /Volumes/OldDrive --output /Volumes/Archive
photo-sorting-hat --db archive.sqlite plan
photo-sorting-hat --db archive.sqlite list --status review
photo-sorting-hat --db archive.sqlite resolve HASH keep_all
photo-sorting-hat --db archive.sqlite export
photo-sorting-hat --db archive.sqlite resume
photo-sorting-hat --db archive.sqlite report --output archive-report.jsonl
```

Use `plan --move-files` to explicitly select Move, or set `"operation": "move"` in a recipe. Nothing is removed during scan or plan. A changed or unavailable source blocks its export and is retained.

`--json` prints structured progress on stderr; command results use JSON on stdout. Reports stream JSON Lines. `list --after ID --limit 100 --status planned` uses keyset pagination. `resume` resumes a scan before planning, or an export after a complete plan. Ctrl-C safely cancels a job.

```sh
photo-sorting-hat recipe > my-recipe.json
photo-sorting-hat --db archive.sqlite plan --recipe my-recipe.json
photo-sorting-hat formats
```

Recipe tokens: `{year}`, `{month}`, `{day}`, `{date}`, `{timestamp}`, `{camera}`, `{make}`, `{source_folder}`, `{event_name}`, `{project}`, `{stem}`. `{event_name}` uses the directory immediately below an `Events` folder and disappears cleanly when no event is available. Templates are relative and cannot escape the chosen output. File extensions are preserved. Collisions receive a `__N` suffix shared by the photo and its sidecars.

Folder keywords discard hidden, percent-encoded, and hash-like mixed identifiers automatically while preserving meaningful numeric and numbered names such as `1999` and `004. Selected Photos`.

Capture dates prefer original EXIF timestamps, then valid creation timestamps. Camera clock times without a timezone are preserved and flagged. Missing dates go to `Undated`; the explicit filesystem fallback uses modification time in UTC. No geocoding or inferred location is performed.

## Important preview boundaries

- macOS preview builds use ad-hoc signatures and are **not Apple-notarized**. See [installation and releases](docs/RELEASING.md).
- Format support means metadata support, not guaranteed full-image decoding or Lightroom/Photos import support. The initial review UI displays filenames and metadata rather than decoded thumbnails. See [compatibility](docs/COMPATIBILITY.md).
- No perceptual similarity matching, destructive duplicate deletion, AI tagging, direct Lightroom catalog edits, or Apple Photos library mutation.
- Source directory symlinks are skipped. Destination symlinks are rejected. Newline-containing and non-UTF-8 paths are reported rather than silently misinterpreted.
- Unknown/new camera formats require updating the bundled ExifTool and compatibility allowlist.
- Output bundles publish one file at a time with atomic no-replace renames. A crash between sidecar and image publication is recovered from the journal. Do not import a still-running export into Lightroom.
- Interrupted pre-publication staging can leave hidden temporary directories under `.photo-hat-staging`. Keep them while resuming; they are not additional archive originals. Never remove staging during an active job.

See [architecture](docs/ARCHITECTURE.md), [validation](docs/VALIDATION.md), and [third-party notices](docs/THIRD_PARTY.md).
