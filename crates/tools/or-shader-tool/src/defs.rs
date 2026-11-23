//! Building a shader definition database, reading one back, and diffing two.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{
    extract_keys, ExtractionProgress, Gta5RetailCryptoProvider, RpfCryptoProvider,
};
use or_io::gta5::shaders::fxc::GrcEffect;
use or_io::gta5::shaders::shader_defs::{ShaderDefinitions, UnresolvedPreset};
use or_io::gta5::shaders::sps::SpsPreset;
use or_title::install::shader_defs::load_shader_definitions;
use or_title::install::GtaInstall;

#[derive(Debug)]
pub struct DefsBuild {
    pub definitions: ShaderDefinitions,
    pub unresolved: Vec<UnresolvedPreset>,
    pub unreadable: Vec<(String, String)>,
    pub incomplete: Vec<(String, usize)>,
}

#[derive(Debug, thiserror::Error)]
pub enum DefsError {
    #[error("input not found: {0}")]
    MissingInput(PathBuf),
    #[error("no `.fxc` under {0}")]
    NoEffects(PathBuf),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Extract(#[from] or_title::install::shaders::ShaderExtractError),
    #[error("key extraction from `{exe}` failed: {source}")]
    Keys {
        exe: PathBuf,
        #[source]
        source: or_io::IoError,
    },
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

/// Build from a directory holding loose `.fxc` and `.sps`.
pub fn build_from_directory(dir: &Path, title: &str, target: &str) -> Result<DefsBuild, DefsError> {
    if !dir.is_dir() {
        return Err(DefsError::MissingInput(dir.to_path_buf()));
    }

    let mut effects = Vec::new();
    let mut presets = Vec::new();
    let mut unreadable = Vec::new();
    let mut incomplete = Vec::new();

    for entry in walk(dir)? {
        let Some(stem) = entry
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
        else {
            continue;
        };
        match entry
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .as_deref()
        {
            Some("fxc") => {
                let bytes = std::fs::read(&entry)?;
                match GrcEffect::parse_counting(&bytes) {
                    Ok((effect, consumed)) => {
                        if consumed < bytes.len() {
                            incomplete.push((stem.clone(), bytes.len() - consumed));
                        }
                        effects.push((stem, effect));
                    }
                    Err(e) => unreadable.push((stem, e.to_string())),
                }
            }
            Some("sps") => {
                let text = std::fs::read_to_string(&entry)?;
                presets.push(SpsPreset::parse(&stem, &text));
            }
            _ => {}
        }
    }

    if effects.is_empty() {
        return Err(DefsError::NoEffects(dir.to_path_buf()));
    }

    let (definitions, unresolved) = ShaderDefinitions::assemble(title, target, &effects, &presets);
    Ok(DefsBuild {
        definitions,
        unresolved,
        unreadable,
        incomplete,
    })
}

/// The decryption an install's own executable yields.
pub(crate) fn install_crypto(root: &Path) -> Result<Arc<dyn RpfCryptoProvider>, DefsError> {
    let exe = ["GTA5.exe", "GTA5_Enhanced.exe", "PlayGTAV.exe"]
        .iter()
        .map(|n| root.join(n))
        .find(|p| p.exists())
        .ok_or_else(|| DefsError::MissingInput(root.join("GTA5.exe")))?;

    let progress = |_: ExtractionProgress| {};
    let keys = extract_keys(&exe, &progress).map_err(|source| DefsError::Keys {
        exe: exe.clone(),
        source,
    })?;
    Ok(Arc::new(Gta5RetailCryptoProvider::new(Arc::new(keys))))
}

/// Build from a game install, which is where the two folders actually live.
pub fn build_from_install(root: &Path, title: &str, target: &str) -> Result<DefsBuild, DefsError> {
    let crypto = install_crypto(root)?;

    let install = GtaInstall {
        root: root.to_path_buf(),
    };
    let load = load_shader_definitions(&install, crypto, title, target)?;
    Ok(DefsBuild {
        definitions: load.definitions,
        unresolved: load.unresolved,
        unreadable: load.unreadable,
        incomplete: load.incomplete,
    })
}

pub fn write(path: &Path, defs: &ShaderDefinitions) -> Result<u64, DefsError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let json = defs.to_json()?;
    std::fs::write(path, &json)?;
    Ok(json.len() as u64)
}

pub fn read(path: &Path) -> Result<ShaderDefinitions, DefsError> {
    Ok(ShaderDefinitions::from_json(&std::fs::read_to_string(
        path,
    )?)?)
}

/// Where one side of a diff came from.
///
/// ⚠ **A rebuild and a stored table have to be the same kind of thing here**,
/// or "regenerate from the install and see whether anything moved" needs a
/// temporary file to express.
pub enum Subject {
    Stored(PathBuf),
    Install(PathBuf),
    Directory(PathBuf),
}

impl Subject {
    /// Classify a path the way `defs build --input` does, plus a `.json` arm.
    pub fn of(path: &Path) -> Self {
        if path.is_file() {
            Subject::Stored(path.to_path_buf())
        } else if path.join("common.rpf").exists() {
            Subject::Install(path.to_path_buf())
        } else {
            Subject::Directory(path.to_path_buf())
        }
    }

    pub fn label(&self) -> String {
        match self {
            Subject::Stored(p) => p.display().to_string(),
            Subject::Install(p) => format!("{} (install)", p.display()),
            Subject::Directory(p) => format!("{} (loose)", p.display()),
        }
    }

    pub fn resolve(&self, title: &str, target: &str) -> Result<ShaderDefinitions, DefsError> {
        match self {
            Subject::Stored(p) => read(p),
            Subject::Install(p) => Ok(build_from_install(p, title, target)?.definitions),
            Subject::Directory(p) => Ok(build_from_directory(p, title, target)?.definitions),
        }
    }
}

/// Every file under `roots` that names a shader, so a coverage run states
/// which formats it covered rather than implying one.
pub fn material_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        if root.is_dir() {
            if let Ok(found) = walk(root) {
                files.extend(found.into_iter().filter(|p| {
                    p.extension().is_some_and(|e| {
                        or_io::gta5::shaders::shader_defs::materials::is_material_bearing(
                            &e.to_string_lossy(),
                        )
                    })
                }));
            }
        } else {
            files.push(root.clone());
        }
    }
    files.sort();
    files.dedup();
    files
}

pub(crate) fn walk(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_with_no_effects_is_refused_rather_than_written_empty() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.sps"), "shader default\n").unwrap();
        assert!(matches!(
            build_from_directory(tmp.path(), "gta5", "gen8"),
            Err(DefsError::NoEffects(_))
        ));
    }

    #[test]
    fn a_missing_directory_is_refused() {
        assert!(matches!(
            build_from_directory(Path::new("/nope/nothing"), "gta5", "gen8"),
            Err(DefsError::MissingInput(_))
        ));
    }

    #[test]
    fn a_written_database_reads_back_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("gta5-gen8.shaderdefs.json");
        let defs = ShaderDefinitions::new("gta5", "gen8");
        write(&path, &defs).unwrap();
        assert_eq!(read(&path).unwrap(), defs);
    }

    /// ⚠ A directory of loose `.fxc` and an install root are told apart by
    /// `common.rpf`, which is the same test `defs build` applies — so
    /// `defs diff` accepts everything `defs build --input` does.
    #[test]
    fn a_side_of_a_diff_is_a_file_an_install_or_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let stored = tmp.path().join("a.shaderdefs.json");
        write(&stored, &ShaderDefinitions::new("gta5", "gen8")).unwrap();
        assert!(matches!(Subject::of(&stored), Subject::Stored(_)));

        let loose = tmp.path().join("loose");
        std::fs::create_dir_all(&loose).unwrap();
        assert!(matches!(Subject::of(&loose), Subject::Directory(_)));

        let install = tmp.path().join("install");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("common.rpf"), b"").unwrap();
        assert!(matches!(Subject::of(&install), Subject::Install(_)));
    }

    /// ⚠ The measurement widens by covering more formats, so the file walk
    /// must not be a `.ydr` filter wearing a general name.
    #[test]
    fn the_walk_collects_every_format_that_names_a_shader_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        for name in [
            "a.ydr", "b.ydd", "c.yft", "d.ypt", "e.ytd", "f.ybn", "g.txt",
        ] {
            std::fs::write(tmp.path().join(name), b"").unwrap();
        }
        let found: Vec<String> = material_files(&[tmp.path().to_path_buf()])
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(found, vec!["a.ydr", "b.ydd", "c.yft", "d.ypt"]);
    }
}
