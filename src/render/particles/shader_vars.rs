//! The shader-var names the preview recognises, and the warning for the ones
//! it does not.
//!
//! The hash the batch builder acts on individually is named here; the
//! rest are checked against the shader definition database, the install's own
//! answer to what a particle shader declares. Recognising a name is not
//! decoding it: what a value means lives in the batch builder.

use or_io::gta5::shaders::shader_defs;

/// What the database says about one `(shader, var)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VarVerdict {
    /// The rule's shader declares it, by name or by the semantic a file
    /// actually carries.
    Declared,
    /// The database describes the shader and not this variable.
    NotDeclared,
    /// The database does not describe this shader at all.
    ShaderUnknown,
    /// ⚠ **Nothing was asked.** No database is installed, so "unrecognised"
    /// is not an answer anything here is entitled to give.
    NoDatabase,
}

pub(super) fn verdict(shader_hash: u32, var_hash: u32) -> VarVerdict {
    if !shader_defs::is_installed() {
        return VarVerdict::NoDatabase;
    }
    if !shader_defs::describes_shader(shader_hash) {
        return VarVerdict::ShaderUnknown;
    }
    if shader_defs::declares(shader_hash, var_hash) {
        VarVerdict::Declared
    } else {
        VarVerdict::NotDeclared
    }
}

/// Pairs already looked at, so the database is consulted once per
/// `(shader, var)` rather than once per rule per frame.
///
/// ⚠ **A pair is only recorded once there is a database to record an answer
/// from.** The load runs on a worker and finishes after the first frames, so
/// a set filled while the slot was empty would suppress every check for the
/// rest of the session.
fn first_look(shader_hash: u32, var_hash: u32) -> bool {
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<std::collections::HashSet<(u32, u32)>>> = OnceLock::new();
    let lock = SEEN.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    lock.lock()
        .map(|mut seen| seen.insert((shader_hash, var_hash)))
        .unwrap_or(true)
}

fn warn_once_no_database(template: &str) {
    use std::sync::{Mutex, OnceLock};
    static SAID: OnceLock<Mutex<bool>> = OnceLock::new();
    let lock = SAID.get_or_init(|| Mutex::new(false));
    if let Ok(mut said) = lock.lock() {
        if !*said {
            *said = true;
            tracing::info!(
                shader = template,
                "no shader definition database is installed, so a rule's shader vars \
                 cannot be checked against what its shader declares; build one with \
                 `or-shader-tool defs build` and point OPENRAGE_SHADER_DEFS at it"
            );
        }
    }
}

pub(super) fn warn_once_unknown_shader_var(rule_name: &str, hash: u32) {
    tracing::warn!(
        rule = rule_name,
        hash = format!("{:#X}", hash),
        "rule binds a shader var its shader does not declare; value will land in \
         `per_batch.shader_vars` positionally and may not reach any sampler"
    );
}

fn warn_once_unknown_shader(rule_name: &str, template: &str) {
    tracing::warn!(
        rule = rule_name,
        shader = template,
        "the shader definition database does not describe this rule's shader, so none \
         of its shader vars can be checked"
    );
}

pub(super) fn warn_unknown_shader_vars(rules: &or_simulation::EmitterRules) {
    let rule = &rules.particle;
    let shader_hash = rule.shader_inst.shader_template_hash_name.get();
    for var in &rules.shader_vars {
        let hash = match var {
            or_io::gta5::particle::ShaderVar::Vector { name_hash, .. } => *name_hash,
            or_io::gta5::particle::ShaderVar::Texture { name_hash, .. } => *name_hash,
            or_io::gta5::particle::ShaderVar::Keyframe { name_hash, .. } => *name_hash,
        };
        match verdict(shader_hash, hash) {
            VarVerdict::Declared => {}
            VarVerdict::NoDatabase => {
                warn_once_no_database(&rule.shader_inst.shader_template_name);
                return;
            }
            VarVerdict::ShaderUnknown => {
                if first_look(shader_hash, 0) {
                    warn_once_unknown_shader(&rule.name, &rule.shader_inst.shader_template_name);
                }
                return;
            }
            VarVerdict::NotDeclared => {
                if first_look(shader_hash, hash) {
                    warn_once_unknown_shader_var(&rule.name, hash);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::gta5::shaders::fxc::{GrcEffect, GrcEffectVariable, GrcVariableType};
    use or_io::gta5::shaders::shader_defs::ShaderDefinitions;
    use or_io::gta5::shaders::sps::SpsPreset;

    /// ⚠ The installed database is process-wide, so two tests that install
    /// and clear it cannot run at the same time.
    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
        GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn effect_with(name: &str, semantic: &str) -> GrcEffect {
        GrcEffect {
            vertex_type: 0,
            presets: Vec::new(),
            programs: Vec::new(),
            cbuffers: Vec::new(),
            variables: vec![GrcEffectVariable {
                var_type: GrcVariableType::Float,
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
            }],
            techniques: Vec::new(),
        }
    }

    /// ⚠ `ptfx_sprite` declares the variable as `wrapLigthtingTerm`, typo
    /// included, and states `WrapLightingTerm`, the spelling a `.ypt` binds, as
    /// its semantic. Neither can be inferred from the other.
    #[test]
    fn a_var_a_ypt_binds_by_semantic_is_declared() {
        let _lock = exclusive();
        shader_defs::clear();
        let (defs, _) = ShaderDefinitions::assemble(
            "gta5",
            "gen8",
            &[(
                "ptfx_sprite".into(),
                effect_with("wrapLigthtingTerm", "WrapLightingTerm"),
            )],
            &[SpsPreset::parse("ptfx_sprite", "shader ptfx_sprite")],
        );
        shader_defs::install(defs);

        let shader = shader_defs::hash_name("ptfx_sprite");
        assert_eq!(
            verdict(shader, shader_defs::hash_name("WrapLightingTerm")),
            VarVerdict::Declared
        );
        assert_eq!(
            verdict(shader, shader_defs::hash_name("NotAVar")),
            VarVerdict::NotDeclared
        );
        assert_eq!(
            verdict(shader_defs::hash_name("nothing"), 1),
            VarVerdict::ShaderUnknown
        );
        shader_defs::clear();
    }

    #[test]
    fn with_no_database_nothing_is_called_unrecognised() {
        let _lock = exclusive();
        shader_defs::clear();
        assert_eq!(
            verdict(shader_defs::hash_name("ptfx_sprite"), 0xDEAD_BEEF),
            VarVerdict::NoDatabase
        );
    }
}
