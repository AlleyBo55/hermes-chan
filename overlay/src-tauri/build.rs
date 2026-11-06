use std::{fs, path::{Path, PathBuf}};

fn copy_file(source: &Path, destination: &Path) {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).expect("create runtime sprite directory");
    }
    fs::copy(source, destination).unwrap_or_else(|error| {
        panic!(
            "copy canonical sprite asset {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    });
    println!("cargo:rerun-if-changed={}", source.display());
}

fn sync_sprite_pack() {
    // sprites/hermeschan-v1 is canonical. Tauri embeds overlay/src, so copy only
    // the runtime package during every build instead of maintaining two packs.
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let project_root = crate_dir.join("../..");
    let source = project_root.join("sprites/hermeschan-v1");
    let frontend = project_root.join("overlay/src");

    copy_file(&source.join("manifest.json"), &frontend.join("hermeschan/manifest.json"));

    let source_atlases = source.join("atlases");
    let runtime_atlases = frontend.join("hermeschan/atlases");
    for entry in fs::read_dir(&source_atlases).expect("read canonical sprite atlases") {
        let entry = entry.expect("read sprite atlas entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("png") {
            copy_file(&path, &runtime_atlases.join(entry.file_name()));
        }
    }
}

fn main() {
    sync_sprite_pack();
    tauri_build::build();
}
