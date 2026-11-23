//! Scenario regions opened as typed roots: every staged `CScenarioPointRegion`
//! against the value tree it was read from, each point's type named through
//! the region's own look-ups, and one point edit that must reach the file as
//! that edit and nothing else.

use crate::corpus::Corpus;
use crate::support;

use or_io::decl::File;
use or_io::hash::literal_string_hash;
use or_io::meta::binary::synth::name_hash;
use or_io::meta::{CScenarioPoint, CScenarioPointRegion, MetaDocument, MetaInstance, Value};

const REGION: &str = "CScenarioPointRegion";

/// Every staged document whose root is a scenario region, with its value tree.
fn regions(what: &str) -> Option<Vec<(String, Vec<u8>, MetaDocument)>> {
    let library = support::schema();
    let found: Vec<_> = Corpus::load()
        .iter()
        .filter_map(|f| {
            let tree = MetaDocument::parse(library, f.bytes.clone()).ok()?;
            (name_hash(&tree.instance.type_name) == literal_string_hash(REGION.as_bytes()))
                .then(|| (f.name.clone(), f.bytes.clone(), tree))
        })
        .collect();
    if found.is_empty() {
        or_test_support::skip_corpus(what, "no scenario region is staged in the metadata corpus");
        return None;
    }
    Some(found)
}

fn field<'a>(inst: &'a MetaInstance, name: &str) -> Option<&'a Value> {
    let key = literal_string_hash(name.as_bytes());
    inst.fields
        .iter()
        .find(|(n, _)| name_hash(n) == key)
        .map(|(_, v)| v)
}

fn field_mut<'a>(inst: &'a mut MetaInstance, name: &str) -> Option<&'a mut Value> {
    let key = literal_string_hash(name.as_bytes());
    inst.fields
        .iter_mut()
        .find(|(n, _)| name_hash(n) == key)
        .map(|(_, v)| v)
}

fn structs(value: Option<&Value>) -> Vec<&MetaInstance> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| match v {
                Value::Struct(inst) | Value::Pointer(Some(inst)) => Some(&**inst),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn inner(value: Option<&Value>) -> Option<&MetaInstance> {
    match value? {
        Value::Struct(inst) | Value::Pointer(Some(inst)) => Some(inst),
        _ => None,
    }
}

/// The tree's points, the region's own then each cluster's, in that order.
fn tree_points(root: &MetaInstance) -> Vec<&MetaInstance> {
    let own = inner(field(root, "Points")).map(|c| structs(field(c, "MyPoints")));
    let clustered = structs(field(root, "Clusters"))
        .into_iter()
        .filter_map(|c| inner(field(c, "Points")))
        .flat_map(|c| structs(field(c, "MyPoints")));
    own.into_iter().flatten().chain(clustered).collect()
}

fn typed_points(region: &CScenarioPointRegion) -> Vec<&CScenarioPoint> {
    region.all_points().map(|(_, p)| p).collect()
}

fn uint(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::UInt(v) => Some(*v),
        Value::Int(v) => u64::try_from(*v).ok(),
        _ => None,
    }
}

/// What the typed record and the tree disagree on; empty when every typed point is the tree's.
fn disagreements(region: &CScenarioPointRegion, root: &MetaInstance) -> Vec<String> {
    let tree = tree_points(root);
    let typed = typed_points(region);
    if tree.len() != typed.len() {
        return vec![format!(
            "{} typed points, {} in the tree",
            typed.len(),
            tree.len()
        )];
    }
    let mut out = Vec::new();
    for (i, (t, p)) in tree.iter().zip(&typed).enumerate() {
        let wants = [
            ("iType", u64::from(p.i_type)),
            ("ModelSetId", u64::from(p.model_set_id)),
            ("iTimeStartOverride", u64::from(p.i_time_start_override)),
            ("iTimeEndOverride", u64::from(p.i_time_end_override)),
            ("iScenarioGroup", u64::from(p.i_scenario_group)),
        ];
        for (name, typed) in wants {
            if uint(field(t, name)) != Some(typed) {
                out.push(format!(
                    "point {i}: `{name}` reads {typed}, the tree holds {:?}",
                    field(t, name)
                ));
            }
        }
        let pos = p.v_position_and_direction;
        if field(t, "vPositionAndDirection") != Some(&Value::Vec4([pos.x, pos.y, pos.z, pos.w])) {
            out.push(format!(
                "point {i}: `vPositionAndDirection` is not the tree's"
            ));
        }
    }
    let lookups = inner(field(root, "LookUps"));
    let names = lookups.map_or(0, |l| match field(l, "TypeNames") {
        Some(Value::Array(items)) => items.len(),
        _ => 0,
    });
    if names != region.look_ups.type_names.len() {
        out.push(format!(
            "{} typed type names, {names} in the tree",
            region.look_ups.type_names.len()
        ));
    }
    let nodes = inner(field(root, "ChainingGraph")).map_or(0, |g| structs(field(g, "Nodes")).len());
    if nodes != region.chaining_graph.nodes.len() {
        out.push(format!(
            "{} typed chaining nodes, {nodes} in the tree",
            region.chaining_graph.nodes.len()
        ));
    }
    out
}

fn first_point(root: &mut MetaInstance) -> Option<&mut MetaInstance> {
    let (Value::Struct(own) | Value::Pointer(Some(own))) = field_mut(root, "Points")? else {
        return None;
    };
    let Value::Array(items) = field_mut(own, "MyPoints")? else {
        return None;
    };
    match items.first_mut()? {
        Value::Struct(p) | Value::Pointer(Some(p)) => Some(p),
        _ => None,
    }
}

/// Raises the first point by a metre and gives it the next type the region names, in the tree.
fn edit_in_tree(root: &mut MetaInstance, types: usize) -> bool {
    let Some(point) = first_point(root) else {
        return false;
    };
    match field_mut(point, "vPositionAndDirection") {
        Some(Value::Vec4(p)) => p[2] += 1.0,
        _ => return false,
    }
    let Some(slot) = field_mut(point, "iType") else {
        return false;
    };
    let next = uint(Some(slot)).map(|t| (t + 1) % types as u64);
    match (slot, next) {
        (Value::UInt(v), Some(next)) => *v = next,
        (Value::Int(v), Some(next)) => *v = next as i64,
        _ => return false,
    }
    true
}

#[test]
fn every_staged_region_opens_typed_and_names_each_points_type() {
    const WHAT: &str = "every_staged_region_opens_typed_and_names_each_points_type";
    let Some(found) = regions(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let mut points = 0usize;
    for (file, bytes, tree) in &found {
        let region = match CScenarioPointRegion::from_bytes(bytes) {
            Ok(region) => region,
            Err(e) => {
                failures.push(format!("{file}: does not open as {REGION}: {e}"));
                continue;
            }
        };
        let wrong = disagreements(&region, &tree.instance);
        if !wrong.is_empty() {
            failures.push(format!(
                "{file}: {}",
                wrong[..wrong.len().min(3)].join("; ")
            ));
            continue;
        }
        for (i, point) in typed_points(&region).into_iter().enumerate() {
            points += 1;
            if region.scenario_type(point).is_none() {
                failures.push(format!(
                    "{file}: point {i} names type {} of {}",
                    point.i_type,
                    region.look_ups.type_names.len()
                ));
            }
        }
        let once = match region.to_bytes() {
            Ok(once) => once,
            Err(e) => {
                failures.push(format!("{file}: an untouched save failed: {e}"));
                continue;
            }
        };
        if tree.to_bytes().ok().as_ref() != Some(&once) {
            failures.push(format!(
                "{file}: an untouched save is not the value tree's own save"
            ));
            continue;
        }
        match CScenarioPointRegion::from_bytes(&once).and_then(|r| r.to_bytes()) {
            Ok(twice) if twice == once => {}
            Ok(_) => failures.push(format!("{file}: a second save differs from the first")),
            Err(e) => failures.push(format!("{file}: the save does not reopen: {e}")),
        }
    }
    assert!(
        points > 0,
        "{} regions are staged and none holds a point",
        found.len()
    );
    assert!(
        failures.is_empty(),
        "{} of {} regions:\n  {}",
        failures.len(),
        found.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn an_edited_point_reaches_the_file_as_that_edit_and_nothing_else() {
    const WHAT: &str = "an_edited_point_reaches_the_file_as_that_edit_and_nothing_else";
    let Some(found) = regions(WHAT) else {
        return;
    };
    let mut failures = Vec::new();
    let mut edited_any = 0usize;
    for (file, bytes, tree) in &found {
        let Ok(region) = CScenarioPointRegion::from_bytes(bytes) else {
            continue;
        };
        let types = region.look_ups.type_names.len();
        if region.points.my_points.is_empty() || types == 0 {
            continue;
        }
        let mut edited = region.clone();
        let point = &mut edited.points.my_points[0];
        point.v_position_and_direction.z += 1.0;
        point.i_type = ((usize::from(point.i_type) + 1) % types) as u8;
        let mut expected = tree.clone();
        assert!(edit_in_tree(&mut expected.instance, types), "{file}");
        edited_any += 1;
        match (edited.to_bytes(), expected.to_bytes()) {
            (Ok(saved), Ok(want)) if saved == want => {
                match CScenarioPointRegion::from_bytes(&saved) {
                    Ok(back) if back == edited => {}
                    Ok(_) => failures.push(format!("{file}: the edit does not reopen as saved")),
                    Err(e) => failures.push(format!("{file}: the edit does not reopen: {e}")),
                }
            }
            (Ok(_), Ok(_)) => failures.push(format!(
                "{file}: the point edit changed more than the point, or left the document's layout"
            )),
            (Err(e), _) => failures.push(format!("{file}: the edit does not save: {e}")),
            (_, Err(e)) => failures.push(format!("{file}: the edited tree does not save: {e}")),
        }
    }
    assert!(
        edited_any > 0,
        "no staged region holds a point of its own to edit"
    );
    assert!(
        failures.is_empty(),
        "{} of {edited_any} edited regions:\n  {}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn a_point_naming_a_type_past_the_look_ups_is_refused_at_save() {
    const WHAT: &str = "a_point_naming_a_type_past_the_look_ups_is_refused_at_save";
    let Some(found) = regions(WHAT) else {
        return;
    };
    let Some(mut region) = found
        .iter()
        .filter_map(|(_, bytes, _)| CScenarioPointRegion::from_bytes(bytes).ok())
        .find(|r| !r.points.my_points.is_empty())
    else {
        or_test_support::skip_corpus(WHAT, "no staged region holds a point of its own");
        return;
    };
    region.points.my_points[0].i_type = region.look_ups.type_names.len() as u8;
    let err = region.to_bytes().expect_err("the point names no type");
    assert!(err.to_string().contains("type"), "{err}");
}
