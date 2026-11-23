use or_io::mrf::{KeyIndex, MvCondition, MvNetworkDef, NodeBody, Test};

use super::*;

/// The network as a tree: each state machine, its states, each state's
/// transitions with their target and conditions, then the nodes by kind.
pub(super) fn draw_move_network(ui: &mut egui::Ui, state: &mut AppState, doc_id: DocumentId) {
    let state: &AppState = state;
    ui.heading("Move Network");
    ui.separator();
    let network = match state.move_network(doc_id) {
        Ok(network) => network,
        Err(why) => {
            ui.colored_label(theme::WARNING, why);
            return;
        }
    };
    let name = |hash: u32| {
        (state.name_index.name_of(hash)).map_or_else(|| hash_text(hash, None), str::to_string)
    };
    let edges = network.edges();
    ui.label(format!(
        "{} nodes, {} requests, {} flags",
        network.nodes.len(),
        network.requests.len(),
        network.flags.len()
    ));

    let machines: Vec<usize> = (0..network.nodes.len())
        .filter(|&i| {
            matches!(
                network.nodes[i].body,
                NodeBody::StateMachine(_) | NodeBody::InlinedStateMachine(_)
            )
        })
        .collect();
    egui::CollapsingHeader::new(format!("State machines ({})", machines.len()))
        .id_salt("mrf_machines")
        .default_open(true)
        .show(ui, |ui| {
            for &m in &machines {
                let machine = &network.nodes[m];
                let ids: Vec<u32> = match &machine.body {
                    NodeBody::StateMachine(sm) => sm.states.iter().map(|s| s.id).collect(),
                    NodeBody::InlinedStateMachine(ism) => ism.states.iter().map(|s| s.id).collect(),
                    _ => continue,
                };
                egui::CollapsingHeader::new(format!(
                    "{} {} ({}, {} states)",
                    ph::TREE_STRUCTURE,
                    name(machine.id),
                    machine.node_type.name(),
                    ids.len()
                ))
                .id_salt(("mrf_machine", m))
                .show(ui, |ui| {
                    for (id, at) in ids.iter().zip(&edges[m].states) {
                        let initial = at.is_some() && *at == edges[m].initial;
                        let Some(s) = *at else {
                            ui.label(format!("{} (no node at its offset)", name(*id)));
                            continue;
                        };
                        let transitions = match &network.nodes[s].body {
                            NodeBody::StateMachine(sm) => &sm.transitions,
                            NodeBody::State(st) => &st.transitions,
                            _ => {
                                ui.label(name(*id));
                                continue;
                            }
                        };
                        egui::CollapsingHeader::new(format!(
                            "{}{} ({} transitions)",
                            name(*id),
                            if initial { " — initial" } else { "" },
                            transitions.len()
                        ))
                        .id_salt(("mrf_state", m, s))
                        .show(ui, |ui| {
                            for (t, target) in transitions.iter().zip(&edges[s].transitions) {
                                let target = target
                                    .map_or_else(|| "?".to_string(), |n| name(network.nodes[n].id));
                                let duration = if t.duration_from_signal() {
                                    format!("{} s", name(t.signal))
                                } else {
                                    format!("{} s", t.duration)
                                };
                                let when = if t.conditions.is_empty() {
                                    "always".to_string()
                                } else {
                                    t.conditions
                                        .iter()
                                        .map(|c| condition(c, network, &name))
                                        .collect::<Vec<_>>()
                                        .join(" and ")
                                };
                                ui.label(format!(
                                    "→ {target}, {duration} {}, when {when}",
                                    t.modifier()
                                ));
                            }
                        });
                    }
                });
            }
        });

    egui::CollapsingHeader::new("Nodes by kind")
        .id_salt("mrf_kinds")
        .show(ui, |ui| {
            for (kind, count) in network.node_counts() {
                egui::CollapsingHeader::new(format!("{} ({count})", kind.name()))
                    .id_salt(("mrf_kind", kind as u16))
                    .show(ui, |ui| {
                        for node in network.nodes.iter().filter(|n| n.node_type == kind) {
                            ui.monospace(name(node.id));
                        }
                    });
            }
        });
}

/// One condition in words, with the request or flag it tests named by its key.
fn condition(c: &MvCondition, network: &MvNetworkDef, name: &dyn Fn(u32) -> String) -> String {
    let bit = |list: &[KeyIndex], index: u32| {
        list.iter()
            .find(|k| k.index == index)
            .map_or_else(|| format!("#{index}"), |k| name(k.key))
    };
    let not = |not_set: u32| if not_set != 0 { "not " } else { "" };
    match c.test {
        Test::InRange {
            signal,
            upper,
            lower,
        } => format!("{} in [{lower}, {upper}]", name(signal)),
        Test::OutOfRange {
            signal,
            upper,
            lower,
        } => {
            format!("{} outside [{lower}, {upper}]", name(signal))
        }
        Test::OnRequest { request, not_set } => {
            format!(
                "request {} {}set",
                bit(&network.requests, request),
                not(not_set)
            )
        }
        Test::OnFlag { flag, not_set } => {
            format!("flag {} {}set", bit(&network.flags, flag), not(not_set))
        }
        Test::AtEvent { event, not_set } => format!("{}event {}", not(not_set), name(event)),
        Test::GreaterThan { signal, value } => format!("{} > {value}", name(signal)),
        Test::GreaterThanEqual { signal, value } => format!("{} >= {value}", name(signal)),
        Test::LessThan { signal, value } => format!("{} < {value}", name(signal)),
        Test::LessThanEqual { signal, value } => format!("{} <= {value}", name(signal)),
        Test::LifetimeGreaterThan { lifetime } => format!("time in state > {lifetime} s"),
        Test::LifetimeLessThan { lifetime } => format!("time in state < {lifetime} s"),
        Test::OnMoveEventTag { tag, not_set } => format!("{}tag {}", not(not_set), name(tag)),
        Test::BoolEquals { signal, value } => format!("{} == {}", name(signal), value != 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_condition_names_the_request_it_tests_by_its_key() {
        let network = MvNetworkDef {
            version: or_io::mrf::VERSION,
            xrefs: Vec::new(),
            requests: vec![KeyIndex {
                key: 0xAB,
                index: 3,
            }],
            flags: Vec::new(),
            nodes: Vec::new(),
            string_table: Vec::new(),
        };
        let name = |hash: u32| format!("h{hash:x}");
        let on = |test| condition(&MvCondition { set: 0, test }, &network, &name);
        assert_eq!(
            on(Test::OnRequest {
                request: 3,
                not_set: 0
            }),
            "request hab set"
        );
        assert_eq!(
            on(Test::OnFlag {
                flag: 1,
                not_set: 1
            }),
            "flag #1 not set"
        );
        assert_eq!(
            on(Test::InRange {
                signal: 0x10,
                upper: 2.0,
                lower: 1.0
            }),
            "h10 in [1, 2]"
        );
    }
}
