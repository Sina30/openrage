//! A scanned GTA VI fragment's physics level in the Outline: each group on its bone, and under it
//! its children.

use or_io::gta6::headerless::Fragment;

use super::*;

pub(super) fn section_fragment(
    ui: &mut egui::Ui,
    f: &Fragment,
    bones: Option<&[&str]>,
    id_root: egui::Id,
) {
    if f.groups.is_empty() {
        return;
    }
    let header = tr!(
        "Physics ({groups} groups, {children} children)",
        groups = f.groups.len(),
        children = f.children.len(),
    );
    collapsing_row(
        ui,
        id_root.with("gta6_physics"),
        true,
        |ui, _| ui.label(header),
        |ui| {
            for (g, group) in f.groups.iter().enumerate() {
                let bone = bones.and_then(|b| b.get(usize::from(group.bone)).copied());
                let bone =
                    bone.map_or_else(|| format!("hash_{:08X}", group.bone_hash), str::to_string);
                collapsing_row(
                    ui,
                    id_root.with(("gta6_group", g)),
                    false,
                    |ui, _| {
                        ui.label(tr!("Group {group}: {bone}", group = g, bone = bone));
                    },
                    |ui| {
                        let children = group.children.start..group.second_set.end;
                        for (i, child) in
                            children.zip(f.children.iter().skip(usize::from(group.children.start)))
                        {
                            let detail = match child.composite {
                                Some(_) => format!("{:.3} kg", child.mass),
                                None => tr!(
                                    "{mass} kg, its own bound",
                                    mass = format!("{:.3}", child.mass)
                                ),
                            };
                            leaf_row(ui, |ui| {
                                ui.label(tr!("Child {child}", child = i));
                                ui.add(detail_label(detail));
                            });
                        }
                    },
                );
            }
        },
    );
    section_wheels(ui, f, bones, id_root);
    section_glass(ui, f, bones, id_root);
}

/// The name of the bone group `g` of `f` is on, else its hash.
fn group_bone(f: &Fragment, bones: Option<&[&str]>, g: u16) -> String {
    let Some(group) = f.groups.get(usize::from(g)) else {
        return format!("{g}");
    };
    let name = bones.and_then(|b| b.get(usize::from(group.bone)).copied());
    name.map_or_else(|| format!("hash_{:08X}", group.bone_hash), str::to_string)
}

/// Each wheel: its bone, group and child, and where the child sits at bind.
fn section_wheels(ui: &mut egui::Ui, f: &Fragment, bones: Option<&[&str]>, id_root: egui::Id) {
    if f.wheels.is_empty() {
        return;
    }
    let header = tr!("Wheels ({count})", count = f.wheels.len());
    collapsing_row(
        ui,
        id_root.with("gta6_wheels"),
        false,
        |ui, _| ui.label(header),
        |ui| {
            for w in &f.wheels {
                let text = tr!(
                    "{bone}: group {group}, child {child}",
                    bone = group_bone(f, bones, w.group),
                    group = w.group,
                    child = w.child,
                );
                let at = f.children.get(usize::from(w.child)).map(|c| c.matrix[3]);
                leaf_row(ui, |ui| {
                    ui.label(text);
                    if let Some([x, y, z, _]) = at {
                        ui.add(detail_label(format!("({x:.3}, {y:.3}, {z:.3})")));
                    }
                });
            }
        },
    );
}

/// The vehicle glass windows, each the child it is, and the glass panes, each its owner group.
fn section_glass(ui: &mut egui::Ui, f: &Fragment, bones: Option<&[&str]>, id_root: egui::Id) {
    if let Some(Ok(glass)) = &f.vehicle_glass {
        let header = tr!(
            "Vehicle glass ({windows} windows)",
            windows = glass.windows.len()
        );
        collapsing_row(
            ui,
            id_root.with("gta6_vehicle_glass"),
            false,
            |ui, _| ui.label(header),
            |ui| {
                for (k, w) in glass.windows.iter().enumerate() {
                    let child = w.child();
                    let group = f.children.get(usize::from(child)).map(|c| c.group);
                    let text = tr!(
                        "Window {window}: child {child}, {bone}",
                        window = k,
                        child = child,
                        bone = group.map(|g| group_bone(f, bones, g)).unwrap_or_default(),
                    );
                    leaf_row(ui, |ui| ui.label(text));
                }
            },
        );
    }
    if let Some(Ok(panes)) = &f.panes {
        let header = tr!("Glass panes ({panes})", panes = panes.len());
        collapsing_row(
            ui,
            id_root.with("gta6_glass_panes"),
            false,
            |ui, _| ui.label(header),
            |ui| {
                for (k, p) in panes.iter().enumerate() {
                    let text = match p.owner {
                        Some(g) => tr!(
                            "Pane {pane}: group {group}, {bone}",
                            pane = k,
                            group = g,
                            bone = group_bone(f, bones, g),
                        ),
                        None => tr!("Pane {pane}: group not read", pane = k),
                    };
                    leaf_row(ui, |ui| ui.label(text));
                }
            },
        );
    }
}
