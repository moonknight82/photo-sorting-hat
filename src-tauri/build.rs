fn main() {
    // Tauri's resource copier overwrites files in place. macOS packaging can leave
    // copied executables read-only, which otherwise breaks the next local build.
    #[cfg(unix)]
    if let Some(resources) = std::env::var_os("OUT_DIR")
        .map(std::path::PathBuf::from)
        .and_then(|out| {
            out.ancestors()
                .nth(3)
                .map(|profile| profile.join("resources"))
        })
    {
        make_writable(&resources);
    }
    tauri_build::build()
}

#[cfg(unix)]
fn make_writable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    if metadata.is_dir() {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                make_writable(&entry.path());
            }
        }
    } else {
        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o200);
        let _ = std::fs::set_permissions(path, permissions);
    }
}
