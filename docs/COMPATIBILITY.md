# Compatibility matrix

The runtime `formats` command intersects ExifTool's supported extensions with the application's photo/video allowlist. Reading support and writing policy are separate. ExifTool's ability to read a file does not imply a RAW decoder or an importer can open truncated or damaged media.

| Format | Metadata read | Output metadata policy | Review preview | Automated fixture result | Lightroom / Photos import |
|---|---|---|---|---|---|
| JPEG | Yes | XMP sidecar + embedded XMP/IPTC | Filename and metadata | Synthetic image round trip | Pending manual import |
| TIFF | Yes | XMP sidecar; TIFF bytes are not rewritten | Filename and metadata | Synthetic image round trip | Pending manual import |
| PNG | Yes | XMP sidecar + embedded XMP | Filename and metadata | Synthetic image round trip | Pending manual import |
| DNG | Yes | XMP sidecar + embedded XMP | Filename and metadata | ExifTool metadata fixture round trip | Pending full-image import |
| Canon CR2 / Nikon NEF | Yes | XMP sidecar; RAW unchanged | Filename and metadata | ExifTool metadata fixtures; byte preservation | Pending full-image import |
| HEIC/HEIF | Yes | XMP sidecar + embedded XMP | Filename and metadata | HEIC metadata fixture round trip | Pending full-image import |
| Other RAW: CR3, ARW, RAF, ORF, RW2, PEF, etc. | If reported by bundled ExifTool | XMP sidecar; RAW unchanged | Filename and metadata | Not individually validated | Pending |
| WebP | If reported by bundled ExifTool | XMP sidecar + embedded XMP | Filename and metadata | Pending | Pending |
| PSD | If reported by bundled ExifTool | XMP sidecar; PSD bytes are not rewritten | Filename and metadata | Pending | Pending |
| AVIF and other allowed photo formats | If reported by bundled ExifTool | XMP sidecar only | Filename and metadata | Pending | Pending |
| MOV / MP4 and allowed video formats | If reported by bundled ExifTool | Unchanged media; existing companions preserved | Filename and metadata | MOV metadata fixture; byte preservation | Pending |

## Lightroom Classic

XMP is XML-based metadata. Adobe documents proprietary RAW metadata in sidecars and embedded metadata for formats including JPEG, TIFF, and DNG. This app writes both sidecar and embedded XMP where policy permits, with flat `dc:subject` keywords. It preserves existing Lightroom hierarchy and develop settings but does not generate hierarchical keywords in v0.1.

Reference: [Adobe metadata basics](https://helpx.adobe.com/lightroom-classic/desktop/organize-photos-in-lightroom-classic/metadata-basics-actions.html).

## Apple Photos

No direct library editing is implemented. Embedded metadata is the intended interoperability path. Do not promise RAW sidecar import: it remains experimental until tested in the target Photos version. Even if a file imports, keyword/date/GPS retention must be checked separately.

Reference: [Apple Photos import overview](https://support.apple.com/en-ie/guide/photos/phta58cd90d3/mac).
