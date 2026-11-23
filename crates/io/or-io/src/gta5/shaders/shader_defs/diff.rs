//! What moved between two shader definition databases.

use std::collections::BTreeMap;

use super::{ShaderDef, ShaderDefinitions, ShaderParamDef, ShaderParamValue, ShaderTrait};

/// One field that says something different on each side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub field: &'static str,
    pub before: String,
    pub after: String,
}

impl Change {
    fn of<T: PartialEq>(
        field: &'static str,
        before: &T,
        after: &T,
        show: impl Fn(&T) -> String,
    ) -> Option<Self> {
        (before != after).then(|| Self {
            field,
            before: show(before),
            after: show(after),
        })
    }
}

/// What happened to one parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamDelta {
    Added { name: String },
    Removed { name: String },
    Changed { name: String, changes: Vec<Change> },
}

impl ParamDelta {
    pub fn name(&self) -> &str {
        match self {
            Self::Added { name } | Self::Removed { name } | Self::Changed { name, .. } => name,
        }
    }
}

/// What happened to one shader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShaderDelta {
    Added {
        name: String,
        parameters: usize,
    },
    Removed {
        name: String,
        parameters: usize,
    },
    Changed {
        name: String,
        changes: Vec<Change>,
        parameters: Vec<ParamDelta>,
    },
}

impl ShaderDelta {
    pub fn name(&self) -> &str {
        match self {
            Self::Added { name, .. } | Self::Removed { name, .. } | Self::Changed { name, .. } => {
                name
            }
        }
    }
}

/// Two databases, and every place they disagree.
///
/// ⚠ A `header` disagreement is reported rather than refused, so a Gen8 and a
/// Gen9 database can be compared; a caller regenerating one install checks the
/// header itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShaderDefsDiff {
    pub header: Vec<Change>,
    pub shaders: Vec<ShaderDelta>,
}

impl ShaderDefsDiff {
    pub fn of(before: &ShaderDefinitions, after: &ShaderDefinitions) -> Self {
        let mut header = Vec::new();
        header.extend(Change::of(
            "schema_version",
            &before.schema_version,
            &after.schema_version,
            u32::to_string,
        ));
        header.extend(Change::of(
            "title",
            &before.title,
            &after.title,
            String::clone,
        ));
        header.extend(Change::of(
            "target",
            &before.target,
            &after.target,
            String::clone,
        ));
        header.extend(Change::of(
            "source_build",
            &before.source_build,
            &after.source_build,
            String::clone,
        ));

        let lhs: BTreeMap<&str, &ShaderDef> = before
            .shaders
            .iter()
            .map(|s| (s.name.as_str(), s))
            .collect();
        let rhs: BTreeMap<&str, &ShaderDef> =
            after.shaders.iter().map(|s| (s.name.as_str(), s)).collect();

        let mut shaders = Vec::new();
        for (name, b) in &lhs {
            match rhs.get(name) {
                None => shaders.push(ShaderDelta::Removed {
                    name: (*name).to_string(),
                    parameters: b.parameters.len(),
                }),
                Some(a) => {
                    if let Some(delta) = shader_delta(b, a) {
                        shaders.push(delta);
                    }
                }
            }
        }
        for (name, a) in &rhs {
            if !lhs.contains_key(name) {
                shaders.push(ShaderDelta::Added {
                    name: (*name).to_string(),
                    parameters: a.parameters.len(),
                });
            }
        }
        shaders.sort_by(|x, y| x.name().cmp(y.name()));

        Self { header, shaders }
    }

    pub fn is_empty(&self) -> bool {
        self.header.is_empty() && self.shaders.is_empty()
    }

    pub fn counts(&self) -> DiffCounts {
        let mut out = DiffCounts::default();
        for delta in &self.shaders {
            match delta {
                ShaderDelta::Added { .. } => out.shaders_added += 1,
                ShaderDelta::Removed { .. } => out.shaders_removed += 1,
                ShaderDelta::Changed { parameters, .. } => {
                    out.shaders_changed += 1;
                    for p in parameters {
                        match p {
                            ParamDelta::Added { .. } => out.parameters_added += 1,
                            ParamDelta::Removed { .. } => out.parameters_removed += 1,
                            ParamDelta::Changed { .. } => out.parameters_changed += 1,
                        }
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffCounts {
    pub shaders_added: usize,
    pub shaders_removed: usize,
    pub shaders_changed: usize,
    pub parameters_added: usize,
    pub parameters_removed: usize,
    pub parameters_changed: usize,
}

fn shader_delta(before: &ShaderDef, after: &ShaderDef) -> Option<ShaderDelta> {
    let mut changes = Vec::new();
    changes.extend(Change::of(
        "effect",
        &before.effect,
        &after.effect,
        String::clone,
    ));
    changes.extend(Change::of(
        "draw_bucket",
        &before.draw_bucket,
        &after.draw_bucket,
        show_bucket,
    ));
    changes.extend(Change::of(
        "vertex_type",
        &before.vertex_type,
        &after.vertex_type,
        |v| format!("0x{v:08X}"),
    ));
    changes.extend(Change::of("traits", &before.traits, &after.traits, |t| {
        show_traits(t)
    }));

    // ⚠ Keyed by name, not by index. A parameter inserted into the middle of an
    // effect shifts every register after it, and an index-keyed diff would
    // report the whole tail as changed rather than the one insertion.
    let lhs: BTreeMap<&str, &ShaderParamDef> = before
        .parameters
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();
    let rhs: BTreeMap<&str, &ShaderParamDef> = after
        .parameters
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect();

    let mut parameters = Vec::new();
    for (name, b) in &lhs {
        match rhs.get(name) {
            None => parameters.push(ParamDelta::Removed {
                name: (*name).to_string(),
            }),
            Some(a) => {
                let pc = param_changes(b, a);
                if !pc.is_empty() {
                    parameters.push(ParamDelta::Changed {
                        name: (*name).to_string(),
                        changes: pc,
                    });
                }
            }
        }
    }
    for name in rhs.keys() {
        if !lhs.contains_key(name) {
            parameters.push(ParamDelta::Added {
                name: (*name).to_string(),
            });
        }
    }
    parameters.sort_by(|x, y| x.name().cmp(y.name()));

    (!changes.is_empty() || !parameters.is_empty()).then(|| ShaderDelta::Changed {
        name: after.name.clone(),
        changes,
        parameters,
    })
}

fn param_changes(before: &ShaderParamDef, after: &ShaderParamDef) -> Vec<Change> {
    let mut out = Vec::new();
    out.extend(Change::of("kind", &before.kind, &after.kind, |k| {
        k.as_str().to_string()
    }));
    out.extend(Change::of(
        "array_len",
        &before.array_len,
        &after.array_len,
        u16::to_string,
    ));
    out.extend(Change::of(
        "register",
        &before.register,
        &after.register,
        u8::to_string,
    ));
    out.extend(Change::of(
        "cbuffer",
        &before.cbuffer,
        &after.cbuffer,
        show_opt,
    ));
    out.extend(Change::of(
        "semantic",
        &before.semantic,
        &after.semantic,
        show_opt,
    ));
    out.extend(Change::of(
        "ui_name",
        &before.ui_name,
        &after.ui_name,
        show_opt,
    ));
    out.extend(Change::of(
        "default",
        &before.default,
        &after.default,
        show_value,
    ));
    out.extend(Change::of(
        "preset_default",
        &before.preset_default,
        &after.preset_default,
        show_value,
    ));
    out
}

fn show_bucket(b: &Option<u32>) -> String {
    b.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}

fn show_opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "-".into())
}

fn show_traits(t: &[ShaderTrait]) -> String {
    if t.is_empty() {
        return "-".into();
    }
    t.iter()
        .map(|x| format!("{}={}", x.name, x.value))
        .collect::<Vec<_>>()
        .join(" ")
}

fn show_value(v: &Option<ShaderParamValue>) -> String {
    match v {
        None => "-".into(),
        Some(ShaderParamValue::Floats(f)) => format!("{f:?}"),
        Some(ShaderParamValue::Uints(u)) => format!("{u:?}"),
    }
}

#[cfg(all(test, feature = "fxc"))]
mod tests {
    use super::*;
    use crate::gta5::shaders::fxc::{GrcEffect, GrcEffectVariable, GrcVariableType};
    use crate::gta5::shaders::sps::SpsPreset;

    fn variable(name: &str, t: GrcVariableType) -> GrcEffectVariable {
        GrcEffectVariable {
            var_type: t,
            count: 1,
            slot: 3,
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

    fn effect(vars: Vec<GrcEffectVariable>) -> GrcEffect {
        GrcEffect {
            vertex_type: 0x59,
            presets: Vec::new(),
            programs: Vec::new(),
            cbuffers: Vec::new(),
            variables: vars,
            techniques: Vec::new(),
        }
    }

    fn db(effects: &[(&str, Vec<GrcEffectVariable>)]) -> ShaderDefinitions {
        let effects: Vec<(String, GrcEffect)> = effects
            .iter()
            .map(|(n, v)| ((*n).to_string(), effect(v.clone())))
            .collect();
        let presets: Vec<SpsPreset> = effects
            .iter()
            .map(|(n, _)| SpsPreset::parse(n, &format!("shader {n}")))
            .collect();
        ShaderDefinitions::assemble("gta5", "gen8", &effects, &presets).0
    }

    #[test]
    fn two_builds_of_the_same_input_diff_to_nothing() {
        let a = db(&[(
            "default",
            vec![variable("Specular", GrcVariableType::Float)],
        )]);
        let b = db(&[(
            "default",
            vec![variable("Specular", GrcVariableType::Float)],
        )]);
        let diff = ShaderDefsDiff::of(&a, &b);
        assert!(diff.is_empty(), "{diff:?}");
        assert_eq!(diff.counts(), DiffCounts::default());
    }

    #[test]
    fn a_shader_that_arrives_or_leaves_is_named() {
        let a = db(&[("default", vec![variable("X", GrcVariableType::Float)])]);
        let b = db(&[("cutout", vec![variable("X", GrcVariableType::Float)])]);
        let diff = ShaderDefsDiff::of(&a, &b);
        assert_eq!(
            diff.shaders,
            vec![
                ShaderDelta::Added {
                    name: "cutout".into(),
                    parameters: 1
                },
                ShaderDelta::Removed {
                    name: "default".into(),
                    parameters: 1
                },
            ]
        );
        let c = diff.counts();
        assert_eq!((c.shaders_added, c.shaders_removed), (1, 1));
    }

    #[test]
    fn a_parameter_that_changes_type_names_both_spellings() {
        let a = db(&[("default", vec![variable("X", GrcVariableType::Float)])]);
        let b = db(&[("default", vec![variable("X", GrcVariableType::Float4)])]);
        let diff = ShaderDefsDiff::of(&a, &b);
        let ShaderDelta::Changed { parameters, .. } = &diff.shaders[0] else {
            panic!("expected a changed shader, got {:?}", diff.shaders[0]);
        };
        assert_eq!(
            parameters,
            &vec![ParamDelta::Changed {
                name: "X".into(),
                changes: vec![Change {
                    field: "kind",
                    before: "float".into(),
                    after: "float4".into()
                }],
            }]
        );
    }

    /// ⚠ An index-keyed diff reports an insertion as "every parameter after it
    /// changed register", which buries the one fact a maintainer needs.
    #[test]
    fn a_parameter_inserted_in_the_middle_is_one_addition_plus_the_registers_it_moved() {
        let mut first = vec![
            variable("A", GrcVariableType::Float),
            variable("C", GrcVariableType::Float),
        ];
        first[0].slot = 0;
        first[1].slot = 1;
        let mut second = vec![
            variable("A", GrcVariableType::Float),
            variable("B", GrcVariableType::Float),
            variable("C", GrcVariableType::Float),
        ];
        second[0].slot = 0;
        second[1].slot = 1;
        second[2].slot = 2;

        let diff = ShaderDefsDiff::of(&db(&[("default", first)]), &db(&[("default", second)]));
        let ShaderDelta::Changed { parameters, .. } = &diff.shaders[0] else {
            panic!("expected a changed shader");
        };
        assert_eq!(parameters.len(), 2);
        assert_eq!(parameters[0], ParamDelta::Added { name: "B".into() });
        assert_eq!(parameters[1].name(), "C");
    }

    #[test]
    fn two_targets_diff_rather_than_refuse() {
        let a = ShaderDefinitions::new("gta5", "gen8");
        let b = ShaderDefinitions::new("gta5", "gen9");
        let diff = ShaderDefsDiff::of(&a, &b);
        assert_eq!(
            diff.header,
            vec![Change {
                field: "target",
                before: "gen8".into(),
                after: "gen9".into()
            }]
        );
        assert!(!diff.is_empty());
    }

    #[test]
    fn a_bucket_a_preset_pins_is_a_change_the_report_names() {
        let mut a = db(&[("default", vec![variable("X", GrcVariableType::Float)])]);
        let b = a.clone();
        a.shaders[0].draw_bucket = Some(3);
        let diff = ShaderDefsDiff::of(&a, &b);
        let ShaderDelta::Changed { changes, .. } = &diff.shaders[0] else {
            panic!("expected a changed shader");
        };
        assert_eq!(
            changes,
            &vec![Change {
                field: "draw_bucket",
                before: "3".into(),
                after: "-".into()
            }]
        );
    }
}
