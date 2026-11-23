//! The material editor: what a shader declares, what this material carries,
//! and the difference between the three ways to put a value back.

use or_io::decl::{Gen8, Vec4};
use or_io::gta5::gen8::{EntryData, GrcInstanceData, GrcInstanceDataEntry};
use or_io::gta5::shaders::shader_defs::{self, ShaderParamDef, ShaderParamValue};
use or_io::gta5::{GrmShaderGroup, RmcDrawable, TextureSlot};

use super::*;
use crate::state::{drawable_mut, BaselineValue, Change, MaterialAddress, MaterialBaseline};

/// One thing the user asked for, applied after the read borrow ends.
enum Command {
    SetRegister {
        name_hash: u32,
        index: usize,
        component: usize,
        value: f32,
    },
    SetTexture {
        name_hash: u32,
        name: String,
    },
    ClearTexture(u32),
    RemoveEntry(u32),
    AddParameter(Box<ShaderParamDef>),
    ResetToFile(u32),
    ResetToPreset(u32),
    MoveShader {
        from: usize,
        to: usize,
    },
}

/// What one entry holds, as the editor shows it.
enum Shown {
    Texture(String),
    Registers(Vec<[f32; 4]>),
}

pub(super) fn draw_material(
    ui: &mut egui::Ui,
    state: &mut AppState,
    doc_id: DocumentId,
    owner: usize,
    shader_index: usize,
) {
    let at = MaterialAddress::new(owner, shader_index);

    let Some(doc) = state.document_mut(doc_id) else {
        return;
    };
    let baseline = doc.material_baseline().clone();
    let Some(drawable) = drawable_mut(&mut doc.payload, at.owner) else {
        empty(ui, "This document's materials are not editable");
        return;
    };
    let Some(group) = drawable.shader_group.as_deref() else {
        empty(ui, "Selected shader is no longer valid");
        return;
    };
    let shader_count = group.shaders.len();
    let Some(shader) = group.shaders.get(shader_index).map(|s| (**s).clone()) else {
        empty(ui, "Selected shader is no longer valid");
        return;
    };
    let shown: Vec<(u32, Shown)> = (shader.entries.iter())
        .map(|e| {
            let value = match &e.data {
                EntryData::Texture(slot) => {
                    Shown::Texture(group.slot_name(slot).unwrap_or_default().to_string())
                }
                EntryData::Float(rows) => Shown::Registers(rows.iter().map(row_of).collect()),
            };
            (e.name_hash.get(), value)
        })
        .collect();

    let mut commands: Vec<Command> = Vec::new();
    draw_header_rows(ui, &shader, shader_index, shader_count, &mut commands);
    draw_entries(ui, &shader, &shown, at, &baseline, &mut commands);
    draw_add_row(ui, &shader, &mut commands);
    draw_restorable(ui, &shader, at, &baseline, &mut commands);

    if commands.is_empty() {
        return;
    }
    apply(state, doc_id, at, commands);
}

fn row_of(v: &Vec4) -> [f32; 4] {
    [v.x, v.y, v.z, v.w]
}

fn effect_of(shader: &GrcInstanceData) -> u32 {
    shader.basis_hash.get()
}

fn declared(shader: &GrcInstanceData) -> Option<Vec<ShaderParamDef>> {
    shader_defs::parameters_of(effect_of(shader))
}

fn draw_header_rows(
    ui: &mut egui::Ui,
    shader: &GrcInstanceData,
    shader_index: usize,
    shader_count: usize,
    commands: &mut Vec<Command>,
) {
    ui.add_space(crate::ui::theme::SPACING_DEFAULT);
    ui.label(
        egui::RichText::new("Material")
            .strong()
            .color(theme::TEXT_PRIMARY),
    );

    let effect = effect_of(shader);
    let described = shader_defs::describes_shader(effect);
    grid(ui, "material_header", |ui| {
        row(ui, "Shader", &shader_name(effect));
        row(ui, "Effect hash", &format!("0x{:08X}", effect));
        row(ui, "Draw bucket", &shader.draw_bucket.to_string());
        row(
            ui,
            "Entries",
            &format!(
                "{} ({} texture)",
                shader.entries.len(),
                shader.textures().count()
            ),
        );
    });

    if !described {
        ui.label(
            egui::RichText::new(
                "No shader definition database describes this shader, so its parameters \
                 are hashes and nothing can be added. Build one with \
                 `or-shader-tool defs build`.",
            )
            .small()
            .color(theme::WARNING),
        );
    }

    // ⚠ Shader order is game-visible, so the control moves the shader and the
    // drawable renumbers every geometry that named one — see
    // `move_shader`.
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("Order {} of {shader_count}", shader_index + 1))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        if ui
            .add_enabled(shader_index > 0, egui::Button::new("↑").small())
            .on_hover_text("Move this shader earlier; every geometry keeps its material")
            .clicked()
        {
            commands.push(Command::MoveShader {
                from: shader_index,
                to: shader_index - 1,
            });
        }
        if ui
            .add_enabled(
                shader_index + 1 < shader_count,
                egui::Button::new("↓").small(),
            )
            .on_hover_text("Move this shader later; every geometry keeps its material")
            .clicked()
        {
            commands.push(Command::MoveShader {
                from: shader_index,
                to: shader_index + 1,
            });
        }
    });
}

fn draw_entries(
    ui: &mut egui::Ui,
    shader: &GrcInstanceData,
    shown: &[(u32, Shown)],
    at: MaterialAddress,
    baseline: &MaterialBaseline,
    commands: &mut Vec<Command>,
) {
    let declarations = declared(shader).unwrap_or_default();
    ui.add_space(crate::ui::theme::SPACING_SNUG);

    for (name_hash, value) in shown {
        let name_hash = *name_hash;
        let def = declarations
            .iter()
            .find(|d| d.name_hash == name_hash || d.semantic_hash == Some(name_hash));
        let label = match def {
            Some(d) => d.ui_name.clone().unwrap_or_else(|| d.name.clone()),
            None => format!("0x{:08X}", name_hash),
        };

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&label).strong());
            if let Some(d) = def {
                ui.label(
                    egui::RichText::new(format!("{} reg {}", d.kind.as_str(), d.register))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
            }
        });

        match value {
            Shown::Texture(name) => {
                let mut text = name.clone();
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::TextEdit::singleline(&mut text).desired_width(180.0))
                        .changed()
                    {
                        commands.push(Command::SetTexture {
                            name_hash,
                            name: text.clone(),
                        });
                    }
                    // ⚠ Two different edits, and the difference reaches the
                    // game: a cleared slot is still declared with nothing
                    // bound, a removed slot lets the shader use its own
                    // default.
                    if ui
                        .small_button("Clear")
                        .on_hover_text("Unbind this slot, keeping it declared")
                        .clicked()
                    {
                        commands.push(Command::ClearTexture(name_hash));
                    }
                    if ui
                        .small_button("Remove")
                        .on_hover_text("Drop the slot entirely; the shader falls back to its own")
                        .clicked()
                    {
                        commands.push(Command::RemoveEntry(name_hash));
                    }
                });
            }
            Shown::Registers(registers) => {
                for (index, register) in registers.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if registers.len() > 1 {
                            ui.label(
                                egui::RichText::new(format!("[{index}]"))
                                    .small()
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                        for (component, current) in register.iter().enumerate() {
                            let mut value = *current;
                            if drag_value(ui, &mut value, |d| d.speed(0.01).max_decimals(4)).inner {
                                commands.push(Command::SetRegister {
                                    name_hash,
                                    index,
                                    component,
                                    value,
                                });
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui
                        .small_button("Remove")
                        .on_hover_text("Drop this parameter; the shader uses its own default")
                        .clicked()
                    {
                        commands.push(Command::RemoveEntry(name_hash));
                    }
                    reset_buttons(ui, name_hash, at, baseline, def, commands);
                });
            }
        }
        ui.add_space(crate::ui::theme::SPACING_TIGHT);
    }
}

/// ⚠ **Three values, not two, and the editor must not collapse them.** The
/// file's value is what this document arrived with; the preset's is what the
/// `.sps` pins; the effect's is what the `.fxc` declares underneath. A
/// material whose preset pins nothing has two, and one whose author typed the
/// declared value has three that happen to agree.
fn reset_buttons(
    ui: &mut egui::Ui,
    name_hash: u32,
    at: MaterialAddress,
    baseline: &MaterialBaseline,
    def: Option<&ShaderParamDef>,
    commands: &mut Vec<Command>,
) {
    let has_file_value = baseline.value(at, name_hash).is_some();
    if ui
        .add_enabled(has_file_value, egui::Button::new("File").small())
        .on_hover_text(if has_file_value {
            "Back to the value this document was loaded or last saved with"
        } else {
            "This parameter was not in the file, so there is no file value"
        })
        .clicked()
    {
        commands.push(Command::ResetToFile(name_hash));
    }

    let preset = def.and_then(|d| {
        d.preset_default
            .as_ref()
            .or(d.default.as_ref())
            .map(|v| (v, d))
    });
    let resettable = preset.and_then(|(v, _)| v.as_float_registers()).is_some();
    let hover = match preset {
        None => "The database declares no default for this parameter".to_string(),
        Some((ShaderParamValue::Uints(_), d)) => format!(
            "`{}` declares an integer default and a constant register has no known \
             integer encoding, so this is not offered",
            d.name
        ),
        Some((_, d)) => match &d.preset_default {
            Some(_) => "Back to what this material's preset pins".to_string(),
            None => "Back to what the effect declares".to_string(),
        },
    };
    if ui
        .add_enabled(resettable, egui::Button::new("Preset").small())
        .on_hover_text(hover)
        .clicked()
    {
        commands.push(Command::ResetToPreset(name_hash));
    }
}

fn draw_add_row(ui: &mut egui::Ui, shader: &GrcInstanceData, commands: &mut Vec<Command>) {
    let Some(declarations) = declared(shader) else {
        return;
    };
    let absent: Vec<&ShaderParamDef> = declarations
        .iter()
        .filter(|d| shader.entry(d.name_hash).is_none())
        .collect();
    if absent.is_empty() {
        return;
    }

    ui.add_space(crate::ui::theme::SPACING_TIGHT);
    ui.separator();
    egui::ComboBox::from_id_salt("material_add_parameter")
        .selected_text(format!("Add one of {} declared…", absent.len()))
        .width(260.0)
        .show_ui(ui, |ui| {
            for def in absent {
                let label = format!(
                    "{}  ({}{})",
                    def.ui_name.as_deref().unwrap_or(&def.name),
                    def.kind.as_str(),
                    if def.is_binding() { ", sampler" } else { "" }
                );
                if ui.selectable_label(false, label).clicked() {
                    commands.push(Command::AddParameter(Box::new(def.clone())));
                }
            }
        });
}

/// Entries the file had that this material no longer carries, offered back.
fn draw_restorable(
    ui: &mut egui::Ui,
    shader: &GrcInstanceData,
    at: MaterialAddress,
    baseline: &MaterialBaseline,
    commands: &mut Vec<Command>,
) {
    let removed = baseline.removed(at, shader);
    if removed.is_empty() {
        return;
    }
    let declarations = declared(shader).unwrap_or_default();
    ui.add_space(crate::ui::theme::SPACING_TIGHT);
    ui.label(
        egui::RichText::new("Removed since load")
            .small()
            .color(theme::TEXT_SECONDARY),
    );
    for name_hash in removed {
        let def = declarations
            .iter()
            .find(|d| d.name_hash == name_hash || d.semantic_hash == Some(name_hash));
        let label = def
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("0x{name_hash:08X}"));
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&label).small());
            let can_restore = def.is_some();
            if ui
                .add_enabled(can_restore, egui::Button::new("Put back").small())
                .on_hover_text(if can_restore {
                    "Add the slot again with the value it had at load"
                } else {
                    "The database does not declare this, so its type and register are unknown"
                })
                .clicked()
            {
                if let Some(def) = def {
                    commands.push(Command::AddParameter(Box::new(def.clone())));
                    commands.push(Command::ResetToFile(name_hash));
                }
            }
        });
    }
}

fn apply(state: &mut AppState, doc_id: DocumentId, at: MaterialAddress, commands: Vec<Command>) {
    let mut steps = Vec::new();
    let mut failure: Option<String> = None;

    {
        let Some(doc) = state.document_mut(doc_id) else {
            return;
        };
        let baseline = doc.material_baseline().clone();
        let Some(drawable) = drawable_mut(&mut doc.payload, at.owner) else {
            return;
        };
        let mut before = None;

        for command in commands {
            if let Command::MoveShader { from, to } = command {
                match move_shader(drawable, from, to) {
                    Ok(()) => {
                        let owner = at.owner;
                        let moved = Change::MoveShader { owner, from, to };
                        steps.push(("Move a material".to_string(), moved));
                    }
                    Err(e) => failure = Some(e),
                }
                continue;
            }

            let Some(group) = drawable.shader_group.as_deref_mut() else {
                return;
            };
            if group.shaders.get(at.shader as usize).is_none() {
                return;
            }
            let shader_index = at.shader as usize;
            let label = label(&command);
            let was = before
                .take()
                .unwrap_or_else(|| (label, group.shaders[shader_index].clone()));
            before = Some(was);
            let outcome = match command {
                Command::MoveShader { .. } => unreachable!("handled above"),
                Command::SetRegister {
                    name_hash,
                    index,
                    component,
                    value,
                } => {
                    let shader = &mut group.shaders[shader_index];
                    let mut registers = match shader.entry(name_hash).map(|e| &e.data) {
                        Some(EntryData::Float(rows)) => rows.iter().map(row_of).collect(),
                        _ => Vec::new(),
                    };
                    if let Some(register) = registers.get_mut(index) {
                        register[component] = value;
                    }
                    set_registers(shader, name_hash, registers)
                }
                Command::SetTexture { name_hash, name } => {
                    set_texture(group, shader_index, name_hash, &name)
                }
                Command::ClearTexture(name_hash) => set_texture(group, shader_index, name_hash, ""),
                Command::RemoveEntry(name_hash) => {
                    remove_entry(&mut group.shaders[shader_index], name_hash)
                }
                Command::AddParameter(def) => add_parameter(&mut group.shaders[shader_index], &def),
                Command::ResetToFile(name_hash) => match baseline.value(at, name_hash) {
                    Some(BaselineValue::Texture(name)) => {
                        set_texture(group, shader_index, name_hash, name)
                    }
                    Some(BaselineValue::Registers(registers)) => set_registers(
                        &mut group.shaders[shader_index],
                        name_hash,
                        registers.clone(),
                    ),
                    None => Ok(()),
                },
                Command::ResetToPreset(name_hash) => {
                    let shader = &mut group.shaders[shader_index];
                    let registers =
                        shader_defs::describe(effect_of(shader), name_hash).and_then(|d| {
                            d.preset_default
                                .as_ref()
                                .or(d.default.as_ref())
                                .and_then(ShaderParamValue::as_float_registers)
                        });
                    match registers {
                        Some(registers) => set_registers(shader, name_hash, registers),
                        None => Ok(()),
                    }
                }
            };
            match outcome {
                Ok(()) => steps.extend(before.take().map(|(label, shader)| {
                    let (owner, index) = (at.owner, shader_index);
                    let part =
                        Change::part(format!("material {owner} {index}"), shader, move |p| {
                            let group = drawable_mut(p, owner)?.shader_group.as_deref_mut()?;
                            group.shaders.get_mut(index)
                        });
                    (label, part)
                })),
                Err(e) => failure = Some(e),
            }
        }
    }

    if let Some(message) = failure {
        state.show_warning_toast(message);
    }
    for (label, change) in steps {
        state.record(doc_id, label, change);
    }
}

/// What a command did, as the history lists it.
fn label(command: &Command) -> String {
    match command {
        Command::SetRegister { .. } => "Set a material value".to_string(),
        Command::SetTexture { name, .. } => format!("Bind {name}"),
        Command::ClearTexture(_) => "Unbind a texture".to_string(),
        Command::RemoveEntry(_) => "Remove a material parameter".to_string(),
        Command::AddParameter(def) => format!("Add {}", def.name),
        Command::ResetToFile(_) => "Reset to the file's value".to_string(),
        Command::ResetToPreset(_) => "Reset to the preset".to_string(),
        Command::MoveShader { .. } => "Move a material".to_string(),
    }
}

fn entry_mut(
    shader: &mut GrcInstanceData,
    name_hash: u32,
) -> Result<&mut GrcInstanceDataEntry<crate::state::Slot>, String> {
    shader
        .entry_mut(name_hash)
        .ok_or_else(|| format!("this material carries no 0x{name_hash:08X}"))
}

/// Adds an entry for a parameter the database declares, keyed by its declared name, where the engine reads its kind.
fn add_parameter(shader: &mut GrcInstanceData, def: &ShaderParamDef) -> Result<(), String> {
    if shader.entry(def.name_hash).is_some() {
        return Err(format!(
            "this material already carries 0x{:08X}",
            def.name_hash
        ));
    }
    if shader.entries.len() >= usize::from(u8::MAX) {
        return Err(format!(
            "a material holds at most {} entries, which is what its count field can express",
            u8::MAX
        ));
    }
    shader.insert_entry(GrcInstanceDataEntry::declared(def));
    Ok(())
}

/// Drops an entry, so the shader falls back to its own default rather than to a slot with nothing bound.
fn remove_entry(shader: &mut GrcInstanceData, name_hash: u32) -> Result<(), String> {
    let at = (shader.entries.iter())
        .position(|e| e.name_hash.get() == name_hash)
        .ok_or_else(|| format!("this material carries no 0x{name_hash:08X}"))?;
    shader.entries.remove(at);
    Ok(())
}

/// Binds a texture slot to `name`, embedded where the group's dictionary holds it; unbinds it when `name` is empty.
fn set_texture(
    group: &mut GrmShaderGroup<Gen8>,
    shader_index: usize,
    name_hash: u32,
    name: &str,
) -> Result<(), String> {
    let slot = if name.is_empty() {
        TextureSlot::Unbound
    } else {
        group.binding(name)
    };
    let shader = group
        .shaders
        .get_mut(shader_index)
        .ok_or("the material is no longer in its group")?;
    let entry = entry_mut(shader, name_hash)?;
    let EntryData::Texture(bound) = &mut entry.data else {
        return Err(format!("0x{name_hash:08X} is not a texture slot"));
    };
    *bound = slot;
    Ok(())
}

/// Replaces a constant's registers, keeping the span the entry holds.
fn set_registers(
    shader: &mut GrcInstanceData,
    name_hash: u32,
    mut registers: Vec<[f32; 4]>,
) -> Result<(), String> {
    let entry = entry_mut(shader, name_hash)?;
    let EntryData::Float(rows) = &mut entry.data else {
        return Err(format!("0x{name_hash:08X} is not a constant"));
    };
    registers.resize(rows.len(), [0.0; 4]);
    *rows = (registers.into_iter())
        .map(|[x, y, z, w]| Vec4 { x, y, z, w })
        .collect();
    Ok(())
}

/// Moves a shader within the group and renumbers every model naming one.
fn move_shader(drawable: &mut RmcDrawable<Gen8>, from: usize, to: usize) -> Result<(), String> {
    let count = (drawable.shader_group.as_deref())
        .ok_or("this drawable has no shader group to reorder")?
        .shaders
        .len();
    if from >= count || to >= count {
        return Err(format!(
            "cannot move shader {from} to {to}: this group holds {count}"
        ));
    }
    drawable.move_shader(from, to);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::decl::Hash32;
    use or_io::gta5::shaders::shader_defs::{hash_name, ShaderParamKind};
    use or_io::gta5::GrmModel;

    fn def(name: &str, kind: ShaderParamKind) -> ShaderParamDef {
        ShaderParamDef {
            name: name.to_string(),
            name_hash: hash_name(name),
            kind,
            array_len: 1,
            register: 3,
            cbuffer: None,
            drawable_place: None,
            semantic: None,
            semantic_hash: None,
            ui_name: None,
            default: None,
            preset_default: None,
        }
    }

    fn group_of(shader: GrcInstanceData) -> GrmShaderGroup<Gen8> {
        let mut group = GrmShaderGroup::<Gen8>::default();
        group.shaders.push(Box::new(shader));
        group
    }

    /// ⚠ The reason these are functions rather than `Vec` operations.
    #[test]
    fn a_sampler_joins_the_texture_prefix_and_a_constant_joins_the_tail() {
        let mut shader = GrcInstanceData::default();
        add_parameter(&mut shader, &def("bumpiness", ShaderParamKind::Float)).unwrap();
        add_parameter(
            &mut shader,
            &def("DiffuseSampler", ShaderParamKind::Texture),
        )
        .unwrap();
        assert!(matches!(shader.entries[0].data, EntryData::Texture(_)));
        assert_eq!(shader.textures().count(), 1);
        assert!(add_parameter(&mut shader, &def("bumpiness", ShaderParamKind::Float)).is_err());

        let mut group = group_of(shader);
        assert!(set_texture(&mut group, 0, hash_name("bumpiness"), "x").is_err());
        set_texture(&mut group, 0, hash_name("DiffuseSampler"), "stone").unwrap();
        assert!(matches!(
            group.shaders[0].textures().next(),
            Some(TextureSlot::Reference(r)) if r.name == "stone"
        ));

        remove_entry(&mut group.shaders[0], hash_name("DiffuseSampler")).unwrap();
        assert_eq!(group.shaders[0].textures().count(), 0);
    }

    #[test]
    fn setting_registers_keeps_the_declared_span() {
        let mut shader = GrcInstanceData::default();
        let mut wide = def("matrix", ShaderParamKind::Float4x4);
        wide.register = 0;
        add_parameter(&mut shader, &wide).unwrap();
        set_registers(&mut shader, wide.name_hash, vec![[1.0; 4]]).unwrap();
        assert!(matches!(
            &shader.entries[0].data,
            EntryData::Float(r) if r.len() == 4 && r[0].x == 1.0
        ));
    }

    /// ⚠ Shaders are named by position, so a move renumbers every geometry.
    #[test]
    fn moving_a_shader_follows_every_geometry_that_named_one() {
        let mut drawable = RmcDrawable::<Gen8>::default();
        let mut group = GrmShaderGroup::<Gen8>::default();
        for name in ["a", "b", "c"] {
            group.shaders.push(Box::new(GrcInstanceData {
                basis_hash: Hash32::from_raw(hash_name(name)),
                ..Default::default()
            }));
        }
        drawable.shader_group = Some(Box::new(group));
        drawable.lod_group.lods[0].models.push(Box::new(GrmModel {
            shader_indices: vec![0, 1, 2, 2],
            ..Default::default()
        }));
        move_shader(&mut drawable, 0, 2).unwrap();
        let order: Vec<u32> = (drawable.shader_group.as_ref().unwrap().shaders.iter())
            .map(|s| s.basis_hash.get())
            .collect();
        assert_eq!(order, ["b", "c", "a"].map(hash_name));
        assert_eq!(
            drawable.lod_group.lods[0].models[0].shader_indices,
            vec![2, 0, 1, 1]
        );
        assert!(move_shader(&mut drawable, 0, 9).is_err());
    }

    /// Undone, an edit of one material and a move of another leave the drawable as it opened.
    #[test]
    fn a_material_edit_and_a_move_undo_to_the_drawable_as_it_opened() {
        let bumpiness = def("bumpiness", ShaderParamKind::Float);
        let mut drawable = or_io::gta5::GtaDrawable::<Gen8>::default();
        let mut group = GrmShaderGroup::<Gen8>::default();
        for name in ["a", "b"] {
            let mut shader = GrcInstanceData {
                basis_hash: Hash32::from_raw(hash_name(name)),
                ..Default::default()
            };
            add_parameter(&mut shader, &bumpiness).unwrap();
            group.shaders.push(Box::new(shader));
        }
        drawable.base.shader_group = Some(Box::new(group));
        drawable.base.lod_group.lods[0]
            .models
            .push(Box::new(GrmModel {
                shader_indices: vec![0, 1, 1],
                ..Default::default()
            }));
        let mut state = AppState::new();
        state.documents.clear();
        let payload = DocPayload::Resource(or_io::Asset::Drawable(drawable));
        let doc = crate::state::Document::new(1, Some("probe.ydr".into()), payload);
        state.documents.push(doc);
        state.active_document = Some(1);
        let shown = |state: &AppState| match &state.document(1).unwrap().payload {
            DocPayload::Resource(or_io::Asset::Drawable(d)) => format!("{d:?}"),
            _ => panic!("not a drawable"),
        };
        let opened = shown(&state);

        let set = Command::SetRegister {
            name_hash: bumpiness.name_hash,
            index: 0,
            component: 0,
            value: 5.0,
        };
        apply(&mut state, 1, MaterialAddress::new(0, 1), vec![set]);
        let moved = Command::MoveShader { from: 0, to: 1 };
        apply(&mut state, 1, MaterialAddress::new(0, 0), vec![moved]);
        let edited = shown(&state);
        assert_ne!(edited, opened, "the edits changed nothing");

        assert_eq!(state.undo().as_deref(), Some("Move a material"));
        assert_eq!(state.undo().as_deref(), Some("Set a material value"));
        assert_eq!(shown(&state), opened, "undone, it is not as it opened");
        assert!(!state.document(1).unwrap().dirty);
        while state.redo().is_some() {}
        assert_eq!(shown(&state), edited, "redone, an edit is lost");
    }
}
