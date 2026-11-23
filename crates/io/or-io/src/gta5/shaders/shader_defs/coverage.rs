//! What a database accounts for in real files, and what is left over.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::materials::MaterialRef;
use super::ShaderDefinitions;

/// One parameter hash a file binds and its own shader does not declare.
///
/// ⚠ **The two cases are different defects and the report must not merge
/// them.** `declared_by` non-empty means some *other* shipped effect declares
/// this name, so the database has the name and the material is pointed at the
/// wrong shader. `declared_by` empty means no shipped effect declares it
/// anywhere, which is drift between the effects the game ships and the
/// drawables it ships and cannot be fixed by reading more `.fxc`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Residue {
    pub occurrences: usize,
    pub bindings: usize,
    /// Shaders that bind it without declaring it.
    pub bound_by: BTreeSet<String>,
    /// Shaders in the database that do declare it.
    pub declared_by: Vec<String>,
}

impl Residue {
    pub fn is_undeclared_anywhere(&self) -> bool {
        self.declared_by.is_empty()
    }
}

/// How much of a set of files a database can account for.
#[derive(Debug, Clone, Default)]
pub struct Coverage {
    pub files_read: usize,
    pub files_refused: Vec<(PathBuf, String)>,
    /// Per extension, how many files were read.
    pub files_by_format: BTreeMap<String, usize>,
    pub materials: usize,
    pub entries: usize,
    pub named: usize,
    /// Shader hashes the database does not describe at all, and how often.
    pub unknown_shaders: BTreeMap<u32, usize>,
    pub residue: BTreeMap<u32, Residue>,
}

impl Coverage {
    /// Resolve every material `materials_in` reads from these files against a database.
    pub fn of<E: std::fmt::Display>(
        defs: &ShaderDefinitions,
        files: &[PathBuf],
        mut materials_in: impl FnMut(&Path, &[u8]) -> Result<Vec<MaterialRef>, E>,
    ) -> Self {
        let mut out = Self::default();
        for path in files {
            let Ok(bytes) = std::fs::read(path) else {
                out.files_refused
                    .push((path.clone(), "unreadable".to_string()));
                continue;
            };
            match materials_in(path, &bytes) {
                Ok(materials) => {
                    out.files_read += 1;
                    *out.files_by_format.entry(format_of(path)).or_default() += 1;
                    out.absorb(defs, &materials);
                }
                Err(e) => out.files_refused.push((path.clone(), e.to_string())),
            }
        }
        out.resolve_residue(defs);
        out
    }

    fn absorb(&mut self, defs: &ShaderDefinitions, materials: &[MaterialRef]) {
        for material in materials {
            self.materials += 1;
            self.entries += material.entries.len();

            let Some(shader) = defs.shader(material.shader_hash) else {
                *self
                    .unknown_shaders
                    .entry(material.shader_hash)
                    .or_default() += 1;
                continue;
            };
            for entry in &material.entries {
                if shader.parameter(entry.name_hash).is_some() {
                    self.named += 1;
                    continue;
                }
                let row = self.residue.entry(entry.name_hash).or_default();
                row.occurrences += 1;
                row.bindings += usize::from(entry.binding);
                row.bound_by.insert(shader.name.clone());
            }
        }
    }

    fn resolve_residue(&mut self, defs: &ShaderDefinitions) {
        for (hash, row) in &mut self.residue {
            row.declared_by = defs
                .shaders
                .iter()
                .filter(|s| s.parameter(*hash).is_some())
                .map(|s| s.name.clone())
                .collect();
        }
    }

    pub fn named_fraction(&self) -> f64 {
        if self.entries == 0 {
            return 0.0;
        }
        self.named as f64 / self.entries as f64
    }

    /// The hashes no shipped effect declares anywhere — the drift itself.
    pub fn undeclared_anywhere(&self) -> Vec<(u32, &Residue)> {
        let mut rows: Vec<(u32, &Residue)> = self
            .residue
            .iter()
            .filter(|(_, r)| r.is_undeclared_anywhere())
            .map(|(h, r)| (*h, r))
            .collect();
        rows.sort_by_key(|(h, r)| (std::cmp::Reverse(r.occurrences), *h));
        rows
    }

    /// The hashes some other shipped effect declares — a material pointed at
    /// the wrong shader rather than a name nobody states.
    pub fn declared_by_another_shader(&self) -> Vec<(u32, &Residue)> {
        let mut rows: Vec<(u32, &Residue)> = self
            .residue
            .iter()
            .filter(|(_, r)| !r.is_undeclared_anywhere())
            .map(|(h, r)| (*h, r))
            .collect();
        rows.sort_by_key(|(h, r)| (std::cmp::Reverse(r.occurrences), *h));
        rows
    }

    /// Texture slots with no declaration: the one residue a material editor
    /// cannot fall back on a raw hash for, because the name is the edit.
    pub fn unnamed_bindings(&self) -> usize {
        self.residue.values().map(|r| r.bindings).sum()
    }
}

fn format_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "?".into())
}

#[cfg(all(test, feature = "fxc"))]
mod tests {
    use super::*;
    use crate::gta5::shaders::fxc::{GrcEffect, GrcEffectVariable, GrcVariableType};
    use crate::gta5::shaders::shader_defs::{hash_name, materials::MaterialEntry};
    use crate::gta5::shaders::sps::SpsPreset;

    fn variable(name: &str, t: GrcVariableType) -> GrcEffectVariable {
        GrcEffectVariable {
            var_type: t,
            count: 1,
            slot: 0,
            group: 0,
            name: name.to_string(),
            semantic: name.to_string(),
            offset: 0,
            variant: 0,
            cbuffer: 0,
            annotations: Vec::new(),
            default_floats: Vec::new(),
            default_uints: Vec::new(),
        }
    }

    fn db(effects: &[(&str, &[&str])]) -> ShaderDefinitions {
        let effects: Vec<(String, GrcEffect)> = effects
            .iter()
            .map(|(n, vars)| {
                (
                    (*n).to_string(),
                    GrcEffect {
                        vertex_type: 0,
                        presets: Vec::new(),
                        programs: Vec::new(),
                        cbuffers: Vec::new(),
                        variables: vars
                            .iter()
                            .map(|v| variable(v, GrcVariableType::Float))
                            .collect(),
                        techniques: Vec::new(),
                    },
                )
            })
            .collect();
        let presets: Vec<SpsPreset> = effects
            .iter()
            .map(|(n, _)| SpsPreset::parse(n, &format!("shader {n}")))
            .collect();
        ShaderDefinitions::assemble("gta5", "gen8", &effects, &presets).0
    }

    fn material(shader: &str, entries: &[(&str, bool)]) -> MaterialRef {
        MaterialRef {
            shader_hash: hash_name(shader),
            shader_name: None,
            owner: String::new(),
            declared_texture_count: None,
            entries: entries
                .iter()
                .map(|(n, binding)| MaterialEntry {
                    name_hash: hash_name(n),
                    binding: *binding,
                    register: 0,
                })
                .collect(),
        }
    }

    /// ⚠ "The database has this name under another shader" and "no shipped
    /// effect states this name" are different defects, and a count of
    /// undeclared hashes merges them.
    #[test]
    fn a_name_another_shader_declares_is_not_the_same_residue_as_one_nobody_declares() {
        let defs = db(&[("default", &["Specular"]), ("decal", &["Bumpiness"])]);
        let mut cov = Coverage::default();
        cov.absorb(
            &defs,
            &[material(
                "default",
                &[
                    ("Specular", false),
                    ("Bumpiness", false),
                    ("Nowhere", false),
                ],
            )],
        );
        cov.resolve_residue(&defs);

        assert_eq!(cov.named, 1);
        assert_eq!(cov.entries, 3);

        let elsewhere = cov.declared_by_another_shader();
        assert_eq!(elsewhere.len(), 1);
        assert_eq!(elsewhere[0].0, hash_name("Bumpiness"));
        assert_eq!(elsewhere[0].1.declared_by, vec!["decal".to_string()]);

        let nowhere = cov.undeclared_anywhere();
        assert_eq!(nowhere.len(), 1);
        assert_eq!(nowhere[0].0, hash_name("Nowhere"));
        assert!(nowhere[0].1.bound_by.contains("default"));
    }

    /// ⚠ An unknown shader must not be counted as 0% named: nothing about its
    /// parameters was measured, and averaging them in reads as coverage.
    #[test]
    fn a_shader_the_database_does_not_describe_is_counted_apart_from_the_residue() {
        let defs = db(&[("default", &["Specular"])]);
        let mut cov = Coverage::default();
        cov.absorb(&defs, &[material("stranger", &[("Specular", false)])]);
        cov.resolve_residue(&defs);

        assert_eq!(cov.unknown_shaders.len(), 1);
        assert!(cov.residue.is_empty());
        assert_eq!(cov.named, 0);
    }

    #[test]
    fn a_texture_slot_with_no_declaration_is_counted_as_one() {
        let defs = db(&[("default", &["Specular"])]);
        let mut cov = Coverage::default();
        cov.absorb(
            &defs,
            &[material(
                "default",
                &[("SomeSampler", true), ("Other", false)],
            )],
        );
        cov.resolve_residue(&defs);
        assert_eq!(cov.unnamed_bindings(), 1);
    }
}
