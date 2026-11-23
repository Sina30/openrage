//! What one material parameter is called in the other generation.
//!
//! ⚠ **Enhanced renamed nothing.** A Legacy file addresses a parameter by the
//! effect variable's **declared name**; an Enhanced file addresses the same
//! parameter of the same shader by that variable's **semantic**.
//! `DiffuseSampler` and `DiffuseTex` are one `.fxc` declaration under its two
//! spellings, and so are `specularFalloffMult` / `Specular`, `specMapIntMask` /
//! `SpecularMapIntensityMask` and every other pair the corpus forces.
//!
//! **The table is the [`ShaderDefinitions`] database**, whose `semantic_hash`
//! holds both spellings of every parameter from the one `.fxc` the Legacy
//! install ships, so there is no separate pair list.
//!
//! **It is per shader and must stay so.** In 385 of a retail Legacy install's
//! shaders one parameter's semantic is another parameter's name —
//! `gCSMShadowTextureSamp` states the semantic `gCSMShadowTexture`, which is
//! also a variable — so a flat name↔semantic map would answer with the wrong
//! parameter. [`ParamRemap`] is keyed by `(shader, hash)`.
//!
//! **Reading is two-way and writing is one-way per target.** A material
//! editor resolves either spelling, because
//! [`ShaderDef::parameter`](super::ShaderDef::parameter) tries the name then
//! the semantic. Writing must pick, and which one it picks is a
//! fact about the target: a Legacy entry carries the name — measured over the
//! staged corpus, all 9,871 resolvable drawable entries match it and none
//! matches only a semantic — and an Enhanced one carries the semantic.
//! [`Spelling::of`] is the only place that decision is made.
//!
//! ⚠ **An Enhanced material is keyed the same way a Legacy one is.**
//! `GrcInstanceData::name` is the `.sps` preset name's hash in both targets.
//! Enhanced's extra `preset` field is **not** a second key and must not be
//! read as one: every shipped material carries the same constant `0x6D657461`
//! — a FourCC, not a hash — measured across ten distinct shaders by
//! `the_preset_field_is_a_constant_tag_and_the_name_field_is_the_key`.

use std::collections::HashMap;

use crate::Generation;

use super::{ShaderDefinitions, ShaderParamDef};

/// Which of a parameter's two spellings a target's files carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spelling {
    /// The effect variable's declared name — what Legacy carries.
    Name,
    /// The variable's semantic — what Enhanced carries.
    Semantic,
}

impl Spelling {
    pub fn of(target: Generation) -> Self {
        if target.is_gen9() {
            Self::Semantic
        } else {
            Self::Name
        }
    }

    /// The hash a file of this spelling's target carries for a parameter.
    ///
    /// ⚠ **Falls back to the name**, because a variable with no semantic is
    /// spelled the same in both generations rather than being absent from
    /// one.
    pub fn hash_of(self, param: &ShaderParamDef) -> u32 {
        match self {
            Self::Name => param.name_hash,
            Self::Semantic => param.semantic_hash.unwrap_or(param.name_hash),
        }
    }

    /// The text a file of this spelling's target carries.
    pub fn name_of(self, param: &ShaderParamDef) -> &str {
        match self {
            Self::Name => &param.name,
            Self::Semantic => param.semantic.as_deref().unwrap_or(&param.name),
        }
    }
}

/// Every parameter of every shader under both of its spellings.
#[derive(Debug, Clone, Default)]
pub struct ParamRemap {
    /// `(shader_hash, hash as one target spells it) -> index into the
    /// shader's parameter list`.
    by_gen8: HashMap<(u32, u32), usize>,
    by_gen9: HashMap<(u32, u32), usize>,
    shaders: HashMap<u32, Vec<ShaderParamDef>>,
}

impl ParamRemap {
    pub fn of(defs: &ShaderDefinitions) -> Self {
        let mut out = Self::default();
        for shader in &defs.shaders {
            for (i, param) in shader.parameters.iter().enumerate() {
                out.by_gen8
                    .entry((shader.name_hash, Spelling::Name.hash_of(param)))
                    .or_insert(i);
                out.by_gen9
                    .entry((shader.name_hash, Spelling::Semantic.hash_of(param)))
                    .or_insert(i);
            }
            out.shaders
                .insert(shader.name_hash, shader.parameters.clone());
        }
        out
    }

    fn param(&self, shader_hash: u32, index: usize) -> Option<&ShaderParamDef> {
        self.shaders.get(&shader_hash)?.get(index)
    }

    /// The parameter a file of `from` addresses by `hash`.
    pub fn resolve(
        &self,
        shader_hash: u32,
        hash: u32,
        from: Generation,
    ) -> Option<&ShaderParamDef> {
        let index = match Spelling::of(from) {
            Spelling::Name => *self.by_gen8.get(&(shader_hash, hash))?,
            Spelling::Semantic => *self.by_gen9.get(&(shader_hash, hash))?,
        };
        self.param(shader_hash, index)
    }

    /// The hash a file of `to` would carry for the same parameter.
    ///
    /// ⚠ `Some(hash)` where the two spellings agree, rather than `None`: the
    /// question is what the other generation carries, and for a parameter
    /// with no semantic that is the same hash.
    pub fn translate(
        &self,
        shader_hash: u32,
        hash: u32,
        from: Generation,
        to: Generation,
    ) -> Option<u32> {
        self.resolve(shader_hash, hash, from)
            .map(|p| Spelling::of(to).hash_of(p))
    }

    /// What a file of `to` would call it.
    pub fn translate_name(
        &self,
        shader_hash: u32,
        hash: u32,
        from: Generation,
        to: Generation,
    ) -> Option<&str> {
        self.resolve(shader_hash, hash, from)
            .map(|p| Spelling::of(to).name_of(p))
    }

    /// Shaders described, for a report that has to say what it covered.
    pub fn shader_count(&self) -> usize {
        self.shaders.len()
    }

    /// Parameters whose two spellings differ, which are the rows that do work.
    pub fn renamed(&self) -> usize {
        self.shaders
            .values()
            .flatten()
            .filter(|p| p.semantic_hash.is_some_and(|s| s != p.name_hash))
            .count()
    }
}

#[cfg(all(test, feature = "fxc"))]
mod tests {
    use super::*;
    use crate::gta5::shaders::fxc::{GrcEffect, GrcEffectVariable, GrcVariableType};
    use crate::gta5::shaders::shader_defs::hash_name;
    use crate::gta5::shaders::sps::SpsPreset;

    fn variable(name: &str, semantic: &str, t: GrcVariableType) -> GrcEffectVariable {
        GrcEffectVariable {
            var_type: t,
            count: 1,
            slot: 0,
            group: 0,
            name: name.to_string(),
            semantic: semantic.to_string(),
            offset: 0,
            variant: 0,
            cbuffer: 0,
            annotations: Vec::new(),
            default_floats: Vec::new(),
            default_uints: Vec::new(),
        }
    }

    fn db(shaders: &[(&str, &[(&str, &str)])]) -> ShaderDefinitions {
        let effects: Vec<(String, GrcEffect)> = shaders
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
                            .map(|(name, sem)| variable(name, sem, GrcVariableType::Float))
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

    /// ⚠ The finding the whole module rests on, in one assertion: the two
    /// generations carry two spellings of one declaration.
    #[test]
    fn a_legacy_file_carries_the_name_and_an_enhanced_one_the_semantic() {
        let defs = db(&[("default", &[("DiffuseSampler", "DiffuseTex")])]);
        let remap = ParamRemap::of(&defs);
        let shader = hash_name("default");

        assert_eq!(
            remap.translate(
                shader,
                hash_name("DiffuseSampler"),
                Generation::Gen8,
                Generation::Gen9
            ),
            Some(hash_name("DiffuseTex"))
        );
        assert_eq!(
            remap.translate(
                shader,
                hash_name("DiffuseTex"),
                Generation::Gen9,
                Generation::Gen8
            ),
            Some(hash_name("DiffuseSampler"))
        );
        assert_eq!(
            remap.translate_name(
                shader,
                hash_name("DiffuseSampler"),
                Generation::Gen8,
                Generation::Gen9
            ),
            Some("DiffuseTex")
        );
    }

    /// ⚠ A variable whose semantic is its own name is spelled the same in
    /// both, so the answer is that hash rather than nothing.
    #[test]
    fn a_parameter_with_no_second_spelling_translates_to_itself() {
        let defs = db(&[("default", &[("bumpiness", "bumpiness")])]);
        let remap = ParamRemap::of(&defs);
        assert_eq!(
            remap.translate(
                hash_name("default"),
                hash_name("bumpiness"),
                Generation::Gen8,
                Generation::Gen9
            ),
            Some(hash_name("bumpiness"))
        );
        assert_eq!(remap.renamed(), 0);
    }

    /// ⚠ **Per shader, and this is the case that forces it.** One
    /// parameter's semantic is another's name in 385 of a retail Legacy
    /// install's shaders, so a flat map would answer with the sampler.
    #[test]
    fn a_name_that_is_another_parameters_semantic_stays_that_parameters_name() {
        let defs = db(&[(
            "alpha",
            &[
                ("gCSMShadowTextureSamp", "gCSMShadowTexture"),
                ("gCSMShadowTexture", "gCSMShadowTexture"),
            ],
        )]);
        let remap = ParamRemap::of(&defs);
        let shader = hash_name("alpha");
        assert_eq!(
            remap
                .resolve(shader, hash_name("gCSMShadowTexture"), Generation::Gen8)
                .map(|p| p.name.as_str()),
            Some("gCSMShadowTexture"),
            "the Legacy spelling is the name, and a name outranks another \
             parameter's semantic"
        );
    }

    #[test]
    fn a_shader_the_database_does_not_describe_translates_nothing() {
        let remap = ParamRemap::of(&db(&[("default", &[("a", "b")])]));
        assert_eq!(
            remap.translate(
                hash_name("stranger"),
                hash_name("a"),
                Generation::Gen8,
                Generation::Gen9
            ),
            None
        );
    }
}
