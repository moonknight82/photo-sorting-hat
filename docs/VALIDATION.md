# Validation checklist

Use copies of representative media for all preview validation. Record the app version, operating system, ExifTool version, camera/source format, and result for each run.

## Validation log

- **2026-09-07 — mixed PSD/TIFF batch:** The project owner successfully processed approximately 500 PSD and TIFF files on macOS. Lightroom import and clean-machine installation remain pending.

## Metadata and importer compatibility

- [ ] Export full-size JPEG, TIFF, PNG, DNG, HEIC, CR2, NEF, and MOV/MP4 samples.
- [ ] Confirm capture date, camera metadata, generated keywords, and existing descriptive metadata after export.
- [ ] Import each supported photo sample into Lightroom Classic and record keyword, develop-setting, and sidecar behavior.
- [ ] Import each supported rendered format into Apple Photos and record keyword, date, and location behavior.
- [ ] Test proprietary RAW plus XMP in Apple Photos separately and keep it marked experimental unless the sidecar is honored.
- [ ] Update `COMPATIBILITY.md` with the tested app/importer versions and outcomes.

## Export safety and recovery

- [ ] Run Copy and Move on a representative mixed-media folder and compare source/output hashes.
- [ ] Confirm Move removes a source only after every output companion has been verified.
- [ ] Interrupt scan, copy, metadata write, publication, and move cleanup; resume each job to completion.
- [ ] Disconnect a source drive and destination drive during separate runs and verify useful recovery errors.
- [ ] Exhaust destination space during staging and confirm that all source files remain intact.
- [ ] Change a source after planning and confirm export refuses the changed file.
- [ ] Repeat a completed export and confirm no destination file is overwritten or duplicated unexpectedly.

## Duplicates, paths, and scale

- [ ] Review exact duplicates with identical companions, conflicting XMP edits, and shared sidecars.
- [ ] Exercise RAW/JPEG pairs, basename collisions, Unicode paths, nested outputs, symlinks, and missing dates.
- [ ] Benchmark one million indexed records and publish discovery, planning, pagination, and database-size results.
- [ ] Benchmark a real-media batch containing large PSD/TIFF files and document throughput and memory use.

## Release installation and updates

- [ ] Install Apple Silicon, Intel macOS, and Linux AppImage builds on clean supported systems.
- [ ] Confirm the bundled ExifTool runtime works without separately installed metadata tools.
- [ ] Record the macOS first-launch approval flow for the ad-hoc signed build.
- [ ] Verify offline startup and disabled update checks.
- [ ] Verify a valid signed update, an invalid signature rejection, and deferred installation during an export.
- [ ] Complete an actual version-to-version update before publishing the first general preview.
