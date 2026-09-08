# Third-party notices

Photo Sorting Hat's own source is MIT licensed. Bundled dependencies retain their licenses.

- **ExifTool**, copyright Phil Harvey, distributed under the same terms as Perl (Artistic License 1.0 or GNU GPL). Project: https://exiftool.org/ . Bundles include the complete installed ExifTool Perl source and license texts.
- **Perl**, copyright Larry Wall and contributors, distributed under the Artistic License 1.0 or GNU GPL. Project/source: https://www.perl.org/ . Bundles include its runtime, core modules, embedded copyright notices, and license texts. Platform distributions may include additional module-specific notices in the copied source/POD.
- **Tauri**, MIT or Apache-2.0; **React**, MIT; **SQLite**, public domain. Rust and JavaScript dependency versions are recorded in Cargo.lock and pnpm-lock.yaml.
- Small RAW/video metadata test fixtures originate from ExifTool's `t/images` regression suite and follow its distribution terms. These truncated fixtures establish metadata behavior; they do not establish image decoding or Lightroom/Photos import compatibility.
- `sample.jpg`, `sample.tiff`, and `sample.png` are synthetic solid-color fixtures generated for this project and licensed under MIT.

Before publishing, generate the complete dependency license inventory with `cargo metadata --locked` and the JavaScript package lock. ExifTool/Perl source URLs and exact bundled versions are included in release documentation and metadata/versions.json.
