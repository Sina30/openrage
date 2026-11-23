//! The source of a `.yft`: its common drawable as a glb, the rules that put it back, and the fragment it replaces the geometry of.
//!
//! A fragment is a drawable with collision, physics, cloth and glass around it, and it is its own
//! kind of source because those are not a `.ydr`'s. This is the first version of the layout in
//! [Source_Format_Design.md]: the common drawable is edited and everything else the fragment holds
//! (the extra and cloth drawables, the physics children, the bounds, the glass) stays the base's.
//! The base is the extracted file, kept beside the glb as `base.yft` like a model's, so a folder
//! compiles with no game install.
//!
//! ```text
//! <name>.fragment.src/    asset.json  base.yft
//!                         drawable/    drawable.json  model.glb
//! ```
//!
//! [Source_Format_Design.md]: ../../../../docs/direction/Source_Format_Design.md

use std::path::{Component, Path, PathBuf};

use crate::drawable_values::Edited;
use or_io::decl::{File, Gen8, Gen9};
use or_io::gta5::GtaFragType;
use or_io::Generation;
use sha2::{Digest, Sha256};

use crate::asset::{folder_name, sha256_hex, AssetManifest, How, Origin, Source, MANIFEST};
use crate::json::canonical;
use crate::models::{
    compile_of, compile_target, model_of, problem, read_model, replaced, same_geometries,
    target_of, Compile, Compiled, Extraction, Model, EDIT, MODEL, RULES,
};
use crate::Problem;

/// The `kind` of a fragment's source.
pub const KIND: &str = "fragment";
/// The folder of the common drawable's rules and geometry.
pub const DIR: &str = "drawable";
/// The file a fragment source always holds besides `asset.json`, below its folder.
pub const FIXED: &str = "drawable/drawable.json";
/// The fragment the glb replaces the geometry of.
pub const BASE: &str = "base.yft";

/// Everything a compile reads, so a recook can tell nothing changed before it imports anything.
#[derive(Clone)]
pub struct Inputs {
    manifest: AssetManifest,
    compile: Compile,
    base: Vec<u8>,
    model: Model,
}

/// Writes `yft` into `parent` as `<name>.fragment.src` and returns the folder; nothing is left behind on a refusal.
pub fn extract(
    yft: &[u8],
    name: &str,
    parent: &Path,
    how: &Extraction<'_>,
) -> Result<PathBuf, Vec<Problem>> {
    let target = target_of(yft, "yft")?;
    let model = if target == Generation::Gen9 {
        common_model::<Gen9>(yft, name, how)
    } else {
        common_model::<Gen8>(yft, name, how)
    }?;
    publish(yft, name, target, parent, how, model)
}

fn common_model<T: Edited>(
    bytes: &[u8],
    name: &str,
    how: &Extraction<'_>,
) -> Result<Model, Vec<Problem>>
where
    GtaFragType<T>: File,
{
    let fragment = GtaFragType::<T>::from_bytes(bytes)
        .map_err(|e| vec![Problem::new(format!("cannot read the .yft: {e}"))])?;
    let Some(common) = fragment.common_drawable.as_deref() else {
        return Err(vec![Problem::new(
            "holds no common drawable, so there is nothing to edit",
        )]);
    };
    model_of::<T>(name, &common.base, how)?.ok_or_else(|| {
        vec![Problem::new(
            "its common drawable holds no geometry, so there is nothing to edit",
        )]
    })
}

/// Checks that what an extraction made compiles, then writes it as the fragment's folder.
fn publish(
    base: &[u8],
    name: &str,
    target: Generation,
    parent: &Path,
    how: &Extraction<'_>,
    model: Model,
) -> Result<PathBuf, Vec<Problem>> {
    let origin = Origin {
        how: How::Extracted,
        from_game: how.from_game,
        virtual_path: how.virtual_path.map(str::to_string),
        game_build: None,
        sha256: Some(sha256_hex(base)),
        cooked_sha256: None,
        extra: Default::default(),
    };
    let mut manifest =
        AssetManifest::new(KIND, name, target.slug(), origin).map_err(|p| vec![p])?;
    let compile = Compile {
        mode: EDIT.to_string(),
        base: sha256_hex(base),
    };
    // Built from a struct of two strings, which always serialises.
    manifest.extra.insert(
        "compile".to_string(),
        serde_json::to_value(&compile).unwrap_or_default(),
    );
    let inputs = Inputs {
        manifest,
        compile,
        base: base.to_vec(),
        model,
    };
    let dir = parent.join(folder_name(name, KIND));
    if dir.exists() {
        return Err(vec![Problem::new(format!(
            "{} already exists",
            dir.display()
        ))]);
    }
    // ⚠ A source the compile would refuse is not written: the person finds out now, with the
    // fragment in front of them, and not at their first build.
    inputs.compile()?;
    match write_folder(&dir, &inputs) {
        Ok(()) => Ok(dir),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(vec![Problem::new(format!(
                "cannot write {}: {e}",
                dir.display()
            ))])
        }
    }
}

fn write_folder(dir: &Path, inputs: &Inputs) -> std::io::Result<()> {
    let inside = dir.join(DIR);
    std::fs::create_dir_all(&inside)?;
    std::fs::write(dir.join(MANIFEST), inputs.manifest.to_text())?;
    std::fs::write(dir.join(BASE), &inputs.base)?;
    // Plain data of strings and flags: serialising it cannot fail.
    std::fs::write(
        inside.join(RULES),
        canonical(&inputs.model.rules).unwrap_or_default(),
    )?;
    std::fs::write(inside.join(MODEL), &inputs.model.glb)
}

impl Inputs {
    pub fn read(source: &Source) -> Result<Self, Vec<Problem>> {
        let manifest = source.manifest.clone();
        if manifest.kind != KIND {
            return Err(vec![problem(
                format!("{:?} is not a fragment source", manifest.kind),
                MANIFEST,
            )]);
        }
        let compile = compile_of(&manifest).map_err(|p| vec![p])?;
        let base = std::fs::read(source.dir.join(BASE))
            .map_err(|e| vec![problem(format!("cannot read: {e}"), BASE)])?;
        let model = read_model(&source.dir.join(DIR), DIR)?;
        Ok(Self {
            manifest,
            compile,
            base,
            model,
        })
    }

    /// A digest of every input, whatever its formatting.
    pub fn digest(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        let mut put = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        };
        put(self.manifest.digest_text().as_bytes());
        put(&self.base);
        put(canonical(&self.model.rules).unwrap_or_default().as_bytes());
        put(&self.model.glb);
        hash.finalize().into()
    }

    /// The `.yft` the inputs compile to, reloaded through the readers before it is returned.
    pub fn compile(&self) -> Result<Compiled, Vec<Problem>> {
        let target = compile_target(&self.manifest)?;
        let held = sha256_hex(&self.base);
        if held != self.compile.base {
            return Err(vec![problem(
                format!(
                    "the base holds {held} and compile.base says {}; the base is not the file the glb was made from",
                    self.compile.base
                ),
                BASE,
            )]);
        }
        let (bytes, report) = if target == Generation::Gen9 {
            self.replace::<Gen9>()
        } else {
            self.replace::<Gen8>()
        }?;
        Ok(Compiled {
            file_name: output_name(&self.manifest),
            bytes,
            report,
        })
    }

    fn replace<T: Edited>(&self) -> Result<(Vec<u8>, Vec<String>), Vec<Problem>>
    where
        GtaFragType<T>: File,
    {
        let mut fragment = GtaFragType::<T>::from_bytes(&self.base)
            .map_err(|e| vec![problem(format!("the base does not read: {e}"), BASE)])?;
        let Some(common) = fragment.common_drawable.as_deref_mut() else {
            return Err(vec![problem(
                "the base holds no common drawable to put the glb into",
                BASE,
            )]);
        };
        let (drawable, report) = replaced(&common.base, &self.model, &format!("{DIR}/{MODEL}"))?;
        common.base = drawable;
        let bytes = fragment.to_bytes().map_err(|e| {
            vec![Problem::new(format!(
                "the fragment would not save, and the game would misread it: {e}"
            ))]
        })?;
        let back = GtaFragType::<T>::from_bytes(&bytes).map_err(|e| {
            vec![Problem::new(format!(
                "the compiled .yft does not read back: {e}"
            ))]
        })?;
        if let (Some(is), Some(was)) = (
            back.common_drawable.as_deref(),
            fragment.common_drawable.as_deref(),
        ) {
            same_geometries(&is.base, &was.base, "the .yft")?;
        }
        Ok((bytes, report))
    }
}

/// What the folder `source` names compiles to.
pub fn compile(source: &Source) -> Result<Compiled, Vec<Problem>> {
    Inputs::read(source)?.compile()
}

/// The name of the file a fragment source compiles to.
pub fn output_name(manifest: &AssetManifest) -> String {
    format!("{}.yft", manifest.name)
}

/// Whether a path inside a fragment source, relative to its folder, is a file the compile reads.
pub fn is_input(inside: &Path) -> bool {
    let parts: Vec<&str> = inside
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    match parts.as_slice() {
        [] => true,
        [file] => *file == MANIFEST || *file == BASE,
        [dir, file] => *dir == DIR && (*file == RULES || *file == MODEL),
        _ => false,
    }
}
