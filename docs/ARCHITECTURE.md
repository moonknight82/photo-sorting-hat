# Architecture and recovery

`photo-hat-engine` owns metadata processing, recipes, indexing, and export semantics. Both the `photo-sorting-hat` CLI and Tauri commands call this engine. The UI cannot directly delete source files or execute a shell.

SQLite stores source fingerprints, metadata JSON, folder candidates, sidecar hashes, selected tags, collision reservations, output artifacts, and move-cleanup state. Scans and plans commit in batches of 250. Duplicate analysis materializes equal-size candidates once, hashes their full contents, and groups by SHA-256. Directory traversal and row processing are streamed. One persistent ExifTool subprocess bounds work and avoids process startup per file; responses have a 60-second timeout. Local JSON metadata queries and keyset pagination keep the webview independent of total archive size.

Jobs take an OS file lock on the session database's companion `.lock` file. The desktop admits one job at a time and refuses update installation or session switching while a job is active. Closing an active app requests cancellation first.

## Export states

1. `planned`: compare source size/mtime and any known duplicate hash against the scan. Verify companion hashes.
2. Stage under the destination filesystem. In Move mode, hardlink unchanged RAW/video data when possible; otherwise copy in cancellable chunks. Verify the staged source hash, write metadata, and read back keywords. Flush staged artifacts.
3. `publishing`: commit artifact paths, hashes, and filesystem identities before publishing any output.
4. Publish using macOS/Linux atomic no-replace rename. Resume recognizes previously published files by recorded identity and hash. An unrelated destination is never overwritten.
5. `cleanup`: reverify the entire destination bundle. In Move mode, hash each original immediately before removal, persist each removal, and retain shared sidecars and unselected duplicates.
6. `done`: preserve the manifest for audit and idempotent resume.

Errors are per-file and recorded. A missing source drive, disk-full condition, metadata failure, source change, or conflicting output leaves sources intact until verified cleanup. A partially completed move resumes cleanup without needing to recopy a source already removed.

The database is a local job manifest, not an interchangeable untrusted file format. Do not open session databases from unknown sources. Schema versions are checked; this initial preview uses schema 2 and rejects incompatible schemas.

## Metadata rules

- Source photos and sidecars are read-only until an explicitly selected Move succeeds.
- Proprietary RAW bytes are never edited. Supported rendered-image/DNG output copies receive XMP subjects; JPEG/TIFF also receive compatible IPTC keywords up to IPTC's per-keyword byte limit. Full Unicode keywords remain in XMP.
- Existing XMP is copied and updated in place to preserve editing fields and unknown namespaces. ACR files are copied byte-for-byte. Multiple distinct packets with the same role require source cleanup before scanning.
- RAW/JPEG pairs with identical stems receive separate output stems to avoid one sidecar governing two different media files.
- Video content is preserved. Video sorting uses its recorded creation date; video keyword embedding is deferred, since importer behavior varies by container and application.

Future AI providers should produce keyword candidates for this same review pipeline. They must remain opt-in, and remote calls must never be triggered by offline metadata jobs.
