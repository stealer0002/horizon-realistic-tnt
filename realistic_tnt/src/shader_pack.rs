//! The explosion shader pack ("Realistic TNT: explosions"), installed with the mod: on a client's
//! start the mod writes it as a folder into the game's `shaderpacks/`, and rewrites it when this
//! version of the mod carries a different one. The player still picks it in Options > Video
//! Settings > Shaders; the mod never changes the picked pack. A dedicated server writes nothing.
use std::path::Path;

use mod_api::{Dist, RegistrationContext};

#[path = "shader_pack_files.rs"]
mod files;

/// The pack's folder in `shaderpacks/`.
const FOLDER: &str = "realistic_tnt_shockwave";

pub(super) fn install(context: &RegistrationContext) {
    if context.dist() != Dist::Client {
        return;
    }
    let Ok(directory) = context.instance_dir() else {
        return;
    };
    let Some(root) = super::config::native_path(directory) else {
        return;
    };
    let pack = root.join("shaderpacks").join(FOLDER);
    if let Err(error) = write(&pack) {
        eprintln!(
            "[realistic_tnt] Could not install the shader pack {} {}: {error}",
            pack.display(),
            files::VERSION
        );
    }
}

/// Writes the pack's files into `pack`, each only when it differs, through a partial file so the
/// game never reads half of one.
fn write(pack: &Path) -> std::io::Result<()> {
    for (name, text) in files::FILES {
        let path = pack.join(name);
        if std::fs::read(&path).is_ok_and(|old| old == text.as_bytes()) {
            continue;
        }
        std::fs::create_dir_all(path.parent().expect("a file inside the pack"))?;
        let partial = path.with_extension("partial");
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pack_matches_its_source() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../shader/realistic_tnt_shockwave");
        // A build from the .hmod has no repository around it.
        if !source.is_dir() {
            return;
        }
        for (name, text) in files::FILES {
            let on_disk = std::fs::read_to_string(source.join(name))
                .unwrap()
                .replace("\r\n", "\n");
            assert!(on_disk == text, "{name} changed: run python tools/embed_shader.py");
        }
    }

    #[test]
    fn writes_the_pack_and_replaces_an_old_one() {
        let pack = std::env::temp_dir().join(format!("realistic_tnt_pack_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pack);
        std::fs::create_dir_all(pack.join("post")).unwrap();
        std::fs::write(pack.join("post/shockwave.wgsl"), "old").unwrap();
        write(&pack).unwrap();
        write(&pack).unwrap();
        for (name, text) in files::FILES {
            assert_eq!(std::fs::read_to_string(pack.join(name)).unwrap(), text);
        }
        assert!(!pack.join("post/shockwave.partial").exists());
        std::fs::remove_dir_all(&pack).unwrap();
    }
}
