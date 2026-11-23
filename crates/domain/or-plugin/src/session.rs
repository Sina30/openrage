//! A guest that stays loaded: what it registers, the frames it describes, and what it draws.
//!
//! [`Runtime::run`](crate::Runtime::run) runs one export once against one document, which is
//! right for an action and wrong for a panel: a panel is called every frame, and a tool such as a
//! cloth painter holds its model in the guest's memory between frames. A [`Session`] instantiates
//! the guest once, runs its `register` export, and then calls `draw_panel`, `run_command` and
//! `picked` on the same instance, each with a fresh fuel allowance.
//!
//! ⚠ A session reaches the document only through the file's bytes, and only for the call that is
//! handed them ([`Call::with_file`]): the host never holds the editor's document across calls, so
//! the guest's model of it cannot go stale without the guest being told by a new call. It has no
//! described-tree readers and no patch; a plugin that wants those runs as an action.
//!
//! ⚠ As in the runtime, only the functions the grants buy are linked, so an ungranted capability is
//! an unresolved import and not a check that one edit could drop.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::abi::{CodecError, Status};
use crate::host::{SaveCheck, WouldBeInvalid};
use crate::runtime::{
    clear_sentence, deliver, deliver_sentence, explain_link, guest_bytes, memory_of, put_u32,
    refusal_status, Failure, HostError, Limits, Outcome, HOST_MODULE, NO_FILE,
};
use crate::surface::{
    self, CommandSpec, Event, PanelSpec, Pick, Primitive, Registrations, SettingKind, SettingSpec,
    SettingValue, Widget, MAX_PRIMITIVES, MAX_REGISTERED, MAX_TEXT, MAX_WIDGETS, NO_SELECTION,
};
use crate::transaction::{self, Transaction};
use crate::{Capability, Grants, NeverWritable, Permissions, WriteGate};

/// The exports a guest publishes, each taking integers: its panels and commands by the index they were registered at.
const REGISTER: &str = "register";
const DRAW_PANEL: &str = "draw_panel";
const RUN_COMMAND: &str = "run_command";
const PICKED: &str = "picked";

/// One host function of a session, and the capability that buys it.
struct SessionFn {
    name: &'static str,
    needs: Capability,
}

/// Every host function a session links, each with the capability it needs.
const SESSION_FUNCTIONS: &[SessionFn] = &[
    SessionFn {
        name: "register_panel",
        needs: Capability::AddPanels,
    },
    SessionFn {
        name: "draw_widget",
        needs: Capability::AddPanels,
    },
    SessionFn {
        name: "register_command",
        needs: Capability::AddCommands,
    },
    SessionFn {
        name: "declare_setting",
        needs: Capability::AddSettings,
    },
    SessionFn {
        name: "setting_value",
        needs: Capability::AddSettings,
    },
    SessionFn {
        name: "set_overlay",
        needs: Capability::DrawOverlays,
    },
    SessionFn {
        name: "overlay_picking",
        needs: Capability::DrawOverlays,
    },
    SessionFn {
        name: "file_size",
        needs: Capability::ReadFileBytes,
    },
    SessionFn {
        name: "read_file",
        needs: Capability::ReadFileBytes,
    },
    SessionFn {
        name: "write_file",
        needs: Capability::WriteFileBytes,
    },
    SessionFn {
        name: "write_files",
        needs: Capability::WriteFiles,
    },
];

/// What one call of a session may reach: the editor's write gate and save check, and the open file's bytes.
pub struct Call {
    gate: Box<dyn WriteGate>,
    check: Box<dyn SaveCheck>,
    file: Option<Arc<Vec<u8>>>,
}

impl Call {
    /// A call whose writes pass `gate` and then `check`, as the runtime's do.
    pub fn new(gate: impl WriteGate + 'static, check: impl SaveCheck + 'static) -> Self {
        Self {
            gate: Box::new(gate),
            check: Box::new(check),
            file: None,
        }
    }

    /// A call that holds no file and refuses every write.
    pub fn without_a_document() -> Self {
        Self::new(
            NeverWritable("no document is open for this call".into()),
            WouldBeInvalid("no document is open for this call".into()),
        )
    }

    /// The same call serving `file` as the bytes of the file the open document would save.
    pub fn with_file(mut self, file: Arc<Vec<u8>>) -> Self {
        self.file = Some(file);
        self
    }
}

/// One drawn frame of a panel: the widgets the guest described, each as the user left it, and what the call did.
#[derive(Debug, Default, PartialEq)]
pub struct Drawn {
    pub widgets: Vec<Widget>,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Registering,
    Drawing,
}

/// Host state a guest's calls act on.
struct State {
    permissions: Permissions<Box<dyn WriteGate>>,
    check: Option<Box<dyn SaveCheck>>,
    /// The file this call serves, until a write the gates kept replaces it.
    file: Option<Arc<Vec<u8>>>,
    replaced: Option<Vec<u8>>,
    /// The files `write_files` kept in this call, for the embedder to apply together.
    transaction: Option<Transaction>,
    failures: Vec<Failure>,
    phase: Phase,
    registrations: Registrations,
    frame: Vec<Widget>,
    /// What the user did since the last frame, per panel and one per widget id, until a call consumes it.
    events: BTreeMap<usize, BTreeMap<String, Event>>,
    /// The panel `draw_panel` is drawing.
    panel: usize,
    settings: BTreeMap<String, SettingValue>,
    overlay: Vec<Primitive>,
    overlay_epoch: u64,
    takes_clicks: bool,
}

impl State {
    fn new() -> Self {
        Self {
            permissions: Permissions::new(
                Grants::none(),
                Box::new(NeverWritable("no call is running".into())) as Box<dyn WriteGate>,
            ),
            check: None,
            file: None,
            replaced: None,
            transaction: None,
            failures: Vec::new(),
            phase: Phase::Idle,
            registrations: Registrations::default(),
            frame: Vec::new(),
            events: BTreeMap::new(),
            panel: 0,
            settings: BTreeMap::new(),
            overlay: Vec::new(),
            overlay_epoch: 0,
            takes_clicks: false,
        }
    }

    fn note(&mut self, function: &'static str, status: Status, because: Option<String>) {
        if status != Status::Ok {
            self.failures.push(Failure {
                function,
                status,
                because,
            });
        }
    }

    /// The file as the guest last left it.
    fn current_file(&self) -> Option<&[u8]> {
        match &self.replaced {
            Some(bytes) => Some(bytes),
            None => self.file.as_deref().map(Vec::as_slice),
        }
    }
}

/// A guest that stays loaded between calls.
pub struct Session {
    store: wasmi::Store<State>,
    instance: wasmi::Instance,
    limits: Limits,
    grants: Grants,
    /// Why the guest can be called no more: after a trap its memory is not something to run again.
    stopped: Option<String>,
    /// What the guest's `register` did, including each registration the host refused.
    started: Outcome,
}

impl Session {
    /// The host functions `grants` buys a session.
    pub fn functions_for(grants: &Grants) -> Vec<&'static str> {
        SESSION_FUNCTIONS
            .iter()
            .filter(|f| grants.has(f.needs))
            .map(|f| f.name)
            .collect()
    }

    /// Which capability an import name needs, if a session defines it.
    pub fn capability_for(import: &str) -> Option<Capability> {
        SESSION_FUNCTIONS
            .iter()
            .find(|f| f.name == import)
            .map(|f| f.needs)
    }

    /// Instantiate `wasm` with only what `grants` permits and run its `register` export.
    pub fn open(wasm: &[u8], grants: Grants, limits: Limits) -> Result<Self, HostError> {
        let mut config = wasmi::Config::default();
        config.consume_fuel(limits.fuel.is_some());
        let engine = wasmi::Engine::new(&config);
        let module =
            wasmi::Module::new(&engine, wasm).map_err(|e| HostError::NotAModule(e.to_string()))?;
        let mut store = wasmi::Store::new(&engine, State::new());
        let mut linker = wasmi::Linker::<State>::new(&engine);
        link(&mut linker, &grants)?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| {
                explain_link(
                    &module,
                    &Self::functions_for(&grants),
                    Self::capability_for,
                    e,
                )
            })?;
        let mut session = Self {
            store,
            instance,
            limits,
            grants,
            stopped: None,
            started: Outcome::default(),
        };
        session.started =
            session.call(Call::without_a_document(), Phase::Registering, REGISTER, ())?;
        Ok(session)
    }

    /// What the guest's `register` did: every registration the host refused is in its failures, with the sentence for the user.
    pub fn started(&self) -> &Outcome {
        &self.started
    }

    /// What the guest registered.
    pub fn registered(&self) -> &Registrations {
        &self.store.data().registrations
    }

    /// Why the guest can be called no more, once a call has trapped.
    pub fn stopped(&self) -> Option<&str> {
        self.stopped.as_deref()
    }

    /// Run the guest's `draw_panel` for the panel registered at `index`, and return the widgets it described.
    pub fn draw_panel(&mut self, index: usize, call: Call) -> Result<Drawn, HostError> {
        if index >= self.registered().panels.len() {
            return Err(HostError::Guest(format!(
                "no panel is registered at {index}"
            )));
        }
        let state = self.store.data_mut();
        state.frame.clear();
        state.panel = index;
        let outcome = self.call(call, Phase::Drawing, DRAW_PANEL, index as i32);
        // ⚠ What no widget of this frame took is dropped, so a click on a widget the guest stopped
        // drawing does not wait for it to come back; another panel's events wait for its own frame.
        let state = self.store.data_mut();
        state.events.remove(&index);
        let widgets = std::mem::take(&mut state.frame);
        outcome.map(|outcome| Drawn { widgets, outcome })
    }

    /// What the user did to a widget of the panel registered at `panel`, for that panel's next frame to answer with.
    pub fn deliver(&mut self, panel: usize, event: Event) {
        if panel >= self.registered().panels.len() {
            return;
        }
        let event = match event {
            Event::Typed { id, text } => Event::Typed {
                id,
                text: surface::cut(text, MAX_TEXT),
            },
            other => other,
        };
        (self.store.data_mut().events.entry(panel).or_default())
            .insert(event.id().to_string(), event);
    }

    /// Run the guest's `run_command` for the command registered at `index`.
    pub fn run_command(&mut self, index: usize, call: Call) -> Result<Outcome, HostError> {
        if index >= self.registered().commands.len() {
            return Err(HostError::Guest(format!(
                "no command is registered at {index}"
            )));
        }
        self.call(call, Phase::Idle, RUN_COMMAND, index as i32)
    }

    /// Tell the guest what a click in the viewport landed on.
    pub fn picked(&mut self, pick: Pick, call: Call) -> Result<Outcome, HostError> {
        let n = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        let params = (
            n(pick.lod),
            n(pick.model),
            n(pick.geometry),
            n(pick.polygon),
            n(pick.vertex),
        );
        self.call(call, Phase::Idle, PICKED, params)
    }

    /// Set the value of a declared setting, kept to what its kind holds; `None` when no such setting is declared or the value is of another kind.
    pub fn set_setting(&mut self, id: &str, value: SettingValue) -> Option<&SettingValue> {
        let kept = self
            .registered()
            .settings
            .iter()
            .find(|spec| spec.id == id)?
            .kind
            .keep(value)?;
        let settings = &mut self.store.data_mut().settings;
        settings.insert(id.to_string(), kept);
        settings.get(id)
    }

    /// The value of a declared setting: its default until [`Session::set_setting`] changes it.
    pub fn setting(&self, id: &str) -> Option<&SettingValue> {
        self.store.data().settings.get(id)
    }

    /// What the guest last asked to have drawn over the open document.
    pub fn overlay(&self) -> &[Primitive] {
        &self.store.data().overlay
    }

    /// Counts each change of the overlay.
    pub fn overlay_epoch(&self) -> u64 {
        self.store.data().overlay_epoch
    }

    /// Whether the guest asked for the viewport's clicks, which then go to it and select nothing.
    pub fn takes_clicks(&self) -> bool {
        self.store.data().takes_clicks
    }

    fn call<P: wasmi::WasmParams>(
        &mut self,
        call: Call,
        phase: Phase,
        export: &str,
        params: P,
    ) -> Result<Outcome, HostError> {
        if let Some(why) = &self.stopped {
            return Err(HostError::Guest(format!(
                "the plugin stopped earlier: {why}"
            )));
        }
        let func = self
            .instance
            .get_typed_func::<P, ()>(&self.store, export)
            .map_err(|e| HostError::Guest(format!("the plugin has no usable `{export}`: {e}")))?;
        let state = self.store.data_mut();
        state.permissions = Permissions::new(self.grants.clone(), call.gate);
        state.check = Some(call.check);
        state.file = call.file;
        state.replaced = None;
        state.transaction = None;
        state.failures.clear();
        state.phase = phase;
        if let Some(fuel) = self.limits.fuel {
            self.store
                .set_fuel(fuel)
                .map_err(|e| HostError::Guest(e.to_string()))?;
        }

        let ran = func.call(&mut self.store, params);

        let state = self.store.data_mut();
        state.phase = Phase::Idle;
        state.check = None;
        state.file = None;
        let refused = state.permissions.refusals().to_vec();
        let replaced = state.replaced.take();
        let transaction = state.transaction.take();
        let failures = std::mem::take(&mut state.failures);
        match ran {
            Ok(()) => Ok(Outcome {
                touched: replaced.is_some(),
                replaced,
                transaction,
                refused,
                failures,
            }),
            Err(e) => {
                self.stopped = Some(e.to_string());
                if refused.is_empty() {
                    Err(HostError::Guest(e.to_string()))
                } else {
                    Err(HostError::StoppedAfterRefusal {
                        why: e.to_string(),
                        refused,
                    })
                }
            }
        }
    }
}

/// Registers one host function: the closure answers `(Status, Option<String>)`, which is noted and returned as the status alone.
macro_rules! host_fn {
    ($linker:expr, $name:literal, |$caller:ident $(, $arg:ident)*| $body:expr) => {{
        $linker
            .func_wrap(
                HOST_MODULE,
                $name,
                |mut $caller: wasmi::Caller<'_, State> $(, $arg: i32)*| -> i32 {
                    let (status, why): Answered = $body;
                    $caller.data_mut().note($name, status, why);
                    status.code()
                },
            )
            .map_err(|e| HostError::Guest(e.to_string()))?;
    }};
}

fn link(linker: &mut wasmi::Linker<State>, grants: &Grants) -> Result<(), HostError> {
    // ⚠ Only the functions the grants buy are defined, so an ungranted capability is an unresolved
    // import rather than a runtime check.
    for host_fn in SESSION_FUNCTIONS.iter().filter(|f| grants.has(f.needs)) {
        match host_fn.name {
            "register_panel" => host_fn!(linker, "register_panel", |caller, ptr, len| {
                register_panel(&mut caller, ptr, len)
            }),
            "draw_widget" => host_fn!(
                linker,
                "draw_widget",
                |caller, ptr, len, out_ptr, out_cap, out_len| {
                    draw_widget(&mut caller, (ptr, len), (out_ptr, out_cap, out_len))
                }
            ),
            "register_command" => host_fn!(linker, "register_command", |caller, ptr, len| {
                register_command(&mut caller, ptr, len)
            }),
            "declare_setting" => host_fn!(linker, "declare_setting", |caller, ptr, len| {
                declare_setting(&mut caller, ptr, len)
            }),
            "setting_value" => host_fn!(
                linker,
                "setting_value",
                |caller, ptr, len, out_ptr, out_cap, out_len| {
                    setting_value(&mut caller, (ptr, len), (out_ptr, out_cap, out_len))
                }
            ),
            "set_overlay" => host_fn!(linker, "set_overlay", |caller, ptr, len| {
                set_overlay(&mut caller, ptr, len)
            }),
            "overlay_picking" => host_fn!(linker, "overlay_picking", |caller, on| {
                caller.data_mut().takes_clicks = on != 0;
                OK
            }),
            "file_size" => host_fn!(linker, "file_size", |caller, out_ptr| {
                file_size(&mut caller, out_ptr)
            }),
            "read_file" => host_fn!(linker, "read_file", |caller, offset, out_ptr, len| {
                read_file(&mut caller, offset, out_ptr, len)
            }),
            "write_file" => host_fn!(
                linker,
                "write_file",
                |caller, ptr, len, why_ptr, why_cap, why_len| {
                    write_file(&mut caller, (ptr, len), (why_ptr, why_cap, why_len))
                }
            ),
            "write_files" => host_fn!(
                linker,
                "write_files",
                |caller, ptr, len, why_ptr, why_cap, why_len| {
                    write_files(&mut caller, (ptr, len), (why_ptr, why_cap, why_len))
                }
            ),
            other => {
                // Unreachable while `SESSION_FUNCTIONS` and this match agree, and
                // `every_session_function_belongs_to_a_capability` is what keeps them agreeing.
                return Err(HostError::Guest(format!(
                    "`{other}` is declared in SESSION_FUNCTIONS and has no implementation"
                )));
            }
        }
    }
    Ok(())
}

type Answered = (Status, Option<String>);

const OK: Answered = (Status::Ok, None);

fn bare(status: Status) -> Answered {
    (status, None)
}

fn refused(why: impl Into<String>) -> Answered {
    (Status::Refused, Some(why.into()))
}

fn malformed(error: CodecError) -> Answered {
    (error.status(), Some(error.to_string()))
}

/// The bytes at `(ptr, len)` of the guest's memory, with the memory they are in.
fn from_guest(
    caller: &mut wasmi::Caller<'_, State>,
    (ptr, len): (i32, i32),
) -> Result<(wasmi::Memory, Vec<u8>), Answered> {
    let Some(memory) = memory_of(caller) else {
        return Err(bare(Status::BadArgument));
    };
    match guest_bytes(memory, caller, ptr, len) {
        Ok(bytes) => Ok((memory, bytes)),
        Err(status) => Err(bare(status)),
    }
}

fn while_registering(caller: &wasmi::Caller<'_, State>, what: &str) -> Result<(), Answered> {
    match caller.data().phase {
        Phase::Registering => Ok(()),
        _ => Err(refused(format!(
            "{what} are registered while the plugin starts, in its `register` export"
        ))),
    }
}

/// An id is what a command, a panel and a stored setting are keyed by, so it is plain.
fn check_id(id: &str, what: &str) -> Result<(), String> {
    let plain = id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if id.is_empty() || !plain {
        return Err(format!(
            "{what} id `{id}` is empty or holds more than letters, digits, `_`, `-` and `.`"
        ));
    }
    Ok(())
}

fn check_text(text: &str, what: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("{what} is empty, so a user would see nothing"));
    }
    Ok(())
}

/// Registered once, never merged: a second registration of an id is refused, so a guest cannot replace its own entry halfway.
fn fresh<'a>(
    mut ids: impl Iterator<Item = &'a str>,
    count: usize,
    id: &str,
    what: &str,
) -> Result<(), String> {
    if count >= MAX_REGISTERED {
        return Err(format!(
            "a plugin registers at most {MAX_REGISTERED} {what}s"
        ));
    }
    if ids.any(|existing| existing == id) {
        return Err(format!(
            "{what} `{id}` is registered twice; a registration never replaces another"
        ));
    }
    Ok(())
}

fn register_panel(caller: &mut wasmi::Caller<'_, State>, ptr: i32, len: i32) -> Answered {
    if let Err(answered) = while_registering(caller, "panels") {
        return answered;
    }
    let (_, bytes) = match from_guest(caller, (ptr, len)) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let spec = match PanelSpec::decode(&bytes) {
        Ok(spec) => spec,
        Err(e) => return malformed(e),
    };
    let panels = &mut caller.data_mut().registrations.panels;
    let checked = check_id(&spec.id, "the panel")
        .and_then(|()| check_text(&spec.title, "the panel's title"))
        .and_then(|()| {
            fresh(
                panels.iter().map(|p| p.id.as_str()),
                panels.len(),
                &spec.id,
                "panel",
            )
        });
    match checked {
        Ok(()) => {
            panels.push(spec);
            OK
        }
        Err(why) => refused(why),
    }
}

fn register_command(caller: &mut wasmi::Caller<'_, State>, ptr: i32, len: i32) -> Answered {
    if let Err(answered) = while_registering(caller, "commands") {
        return answered;
    }
    let (_, bytes) = match from_guest(caller, (ptr, len)) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let spec = match CommandSpec::decode(&bytes) {
        Ok(spec) => spec,
        Err(e) => return malformed(e),
    };
    let commands = &mut caller.data_mut().registrations.commands;
    let checked = check_id(&spec.id, "the command")
        .and_then(|()| check_text(&spec.label, "the command's label"))
        .and_then(|()| {
            fresh(
                commands.iter().map(|c| c.id.as_str()),
                commands.len(),
                &spec.id,
                "command",
            )
        });
    match checked {
        Ok(()) => {
            commands.push(spec);
            OK
        }
        Err(why) => refused(why),
    }
}

fn declare_setting(caller: &mut wasmi::Caller<'_, State>, ptr: i32, len: i32) -> Answered {
    if let Err(answered) = while_registering(caller, "settings") {
        return answered;
    }
    let (_, bytes) = match from_guest(caller, (ptr, len)) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let spec = match SettingSpec::decode(&bytes) {
        Ok(spec) => spec,
        Err(e) => return malformed(e),
    };
    let state = caller.data_mut();
    let kind_holds = match &spec.kind {
        SettingKind::Number { min, max, default } => {
            [min, max, default].iter().all(|v| v.is_finite())
                && *min <= *default
                && *default <= *max
        }
        _ => true,
    };
    let checked = check_id(&spec.id, "the setting")
        .and_then(|()| check_text(&spec.label, "the setting's label"))
        .and_then(|()| {
            kind_holds
                .then_some(())
                .ok_or_else(|| format!("setting `{}` has a default outside its range", spec.id))
        })
        .and_then(|()| {
            let settings = &state.registrations.settings;
            fresh(
                settings.iter().map(|s| s.id.as_str()),
                settings.len(),
                &spec.id,
                "setting",
            )
        });
    match checked {
        Ok(()) => {
            state
                .settings
                .insert(spec.id.clone(), spec.kind.default_value());
            state.registrations.settings.push(spec);
            OK
        }
        Err(why) => refused(why),
    }
}

fn setting_value(
    caller: &mut wasmi::Caller<'_, State>,
    id: (i32, i32),
    out: (i32, i32, i32),
) -> Answered {
    let (memory, bytes) = match from_guest(caller, id) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let Ok(id) = String::from_utf8(bytes) else {
        return bare(Status::BadArgument);
    };
    let Some(value) = caller.data().settings.get(&id) else {
        return (
            Status::NoSuchField,
            Some(format!("the plugin declared no setting `{id}`")),
        );
    };
    let encoded = value.encode();
    bare(deliver(memory, caller, out, &encoded))
}

/// A widget the guest describes, answered with what the user did to it since the last frame, or the guest's own value.
fn draw_widget(
    caller: &mut wasmi::Caller<'_, State>,
    widget: (i32, i32),
    out: (i32, i32, i32),
) -> Answered {
    if caller.data().phase != Phase::Drawing {
        return refused("widgets are described while a panel is drawn, in `draw_panel`");
    }
    let (memory, bytes) = match from_guest(caller, widget) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let widget = match Widget::decode(&bytes) {
        Ok(widget) => widget,
        Err(e) => return malformed(e),
    };
    if caller.data().frame.len() >= MAX_WIDGETS {
        return refused(format!(
            "a panel's frame holds at most {MAX_WIDGETS} widgets"
        ));
    }
    let panel = caller.data().panel;
    let event = widget
        .id()
        .and_then(|id| caller.data().events.get(&panel)?.get(id));
    let (widget, clicked, answered) = apply(widget, event);
    let answer = match &widget {
        Widget::Label { .. } => Vec::new(),
        Widget::Button { .. } => vec![u8::from(clicked)],
        Widget::Slider { value, .. } => value.to_le_bytes().to_vec(),
        Widget::Checkbox { checked, .. } => vec![u8::from(*checked)],
        Widget::TextField { text, .. } => text.as_bytes().to_vec(),
        Widget::List { selected, .. } => selected.unwrap_or(NO_SELECTION).to_le_bytes().to_vec(),
    };
    let status = deliver(memory, caller, out, &answer);
    // ⚠ Only once the guest has the answer: a buffer too small is retried, and the retry must see the same event.
    if status == Status::Ok {
        let state = caller.data_mut();
        if answered {
            if let (Some(id), Some(events)) = (widget.id(), state.events.get_mut(&panel)) {
                events.remove(id);
            }
        }
        state.frame.push(widget);
    }
    bare(status)
}

/// `widget` as the event for its id leaves it, whether a button was clicked, and whether the event was of its kind.
fn apply(widget: Widget, event: Option<&Event>) -> (Widget, bool, bool) {
    match (widget, event) {
        (button @ Widget::Button { .. }, Some(Event::Clicked { .. })) => (button, true, true),
        (
            Widget::Slider {
                id,
                label,
                min,
                max,
                ..
            },
            Some(Event::Slid { value, .. }),
        ) if value.is_finite() && min <= max => (
            Widget::Slider {
                id,
                label,
                min,
                max,
                value: (*value).clamp(min, max),
            },
            false,
            true,
        ),
        (Widget::Checkbox { id, label, .. }, Some(Event::Checked { checked, .. })) => (
            Widget::Checkbox {
                id,
                label,
                checked: *checked,
            },
            false,
            true,
        ),
        (Widget::TextField { id, label, .. }, Some(Event::Typed { text, .. })) => (
            Widget::TextField {
                id,
                label,
                text: text.clone(),
            },
            false,
            true,
        ),
        (Widget::List { id, items, .. }, Some(Event::Chose { index, .. }))
            if (*index as usize) < items.len() =>
        {
            (
                Widget::List {
                    id,
                    items,
                    selected: Some(*index),
                },
                false,
                true,
            )
        }
        (widget, _) => (widget, false, false),
    }
}

fn set_overlay(caller: &mut wasmi::Caller<'_, State>, ptr: i32, len: i32) -> Answered {
    let (_, bytes) = match from_guest(caller, (ptr, len)) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let primitives = match surface::decode_overlay(&bytes) {
        Ok(primitives) => primitives,
        Err(e) => return malformed(e),
    };
    debug_assert!(primitives.len() <= MAX_PRIMITIVES);
    let state = caller.data_mut();
    if state.overlay != primitives {
        state.overlay = primitives;
        state.overlay_epoch += 1;
    }
    OK
}

fn permits(caller: &mut wasmi::Caller<'_, State>, capability: Capability) -> Result<(), Answered> {
    caller
        .data_mut()
        .permissions
        .permits(capability)
        .map_err(|refused| (refusal_status(&refused.because), Some(refused.to_string())))
}

fn file_size(caller: &mut wasmi::Caller<'_, State>, out_ptr: i32) -> Answered {
    if let Err(answered) = permits(caller, Capability::ReadFileBytes) {
        return answered;
    }
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    let Some(len) = caller.data().current_file().map(<[u8]>::len) else {
        return refused(NO_FILE);
    };
    match put_u32(memory, caller, out_ptr, len) {
        Ok(()) => OK,
        Err(status) => bare(status),
    }
}

/// Copies from the bytes the call was handed into the guest's memory: nothing is built on the host's side.
fn read_file(
    caller: &mut wasmi::Caller<'_, State>,
    offset: i32,
    out_ptr: i32,
    len: i32,
) -> Answered {
    if let Err(answered) = permits(caller, Capability::ReadFileBytes) {
        return answered;
    }
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    let (Ok(offset), Ok(out_ptr), Ok(len)) = (
        usize::try_from(offset),
        usize::try_from(out_ptr),
        usize::try_from(len),
    ) else {
        return bare(Status::BadArgument);
    };
    let (guest, state) = memory.data_and_store_mut(&mut *caller);
    let Some(file) = state.current_file() else {
        return refused(NO_FILE);
    };
    let Some(source) = offset
        .checked_add(len)
        .and_then(|end| file.get(offset..end))
    else {
        return (
            Status::BadArgument,
            Some(format!(
                "the file holds {} bytes, so {len} from {offset} runs past its end",
                file.len()
            )),
        );
    };
    let Some(target) = out_ptr
        .checked_add(len)
        .and_then(|end| guest.get_mut(out_ptr..end))
    else {
        return bare(Status::BadArgument);
    };
    target.copy_from_slice(source);
    OK
}

/// Replaces the file with the guest's bytes if the gates keep them, and gives the guest the sentence for a refusal.
fn write_file(
    caller: &mut wasmi::Caller<'_, State>,
    bytes_span: (i32, i32),
    why: (i32, i32, i32),
) -> Answered {
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    // ⚠ The sentence's slot is checked and cleared before anything is decided, so a bad slot is
    // never the answer to a write that was kept.
    if let Err(status) = clear_sentence(memory, caller, why) {
        return bare(status);
    }
    let answered = replace_file(caller, bytes_span);
    if let (status, Some(sentence)) = &answered {
        if *status != Status::Ok {
            deliver_sentence(memory, caller, why, sentence);
        }
    }
    answered
}

/// Both gates, then the guest's bytes, then the embedder's check of the file; the bytes are kept only once all three agree.
fn replace_file(caller: &mut wasmi::Caller<'_, State>, bytes_span: (i32, i32)) -> Answered {
    // Both the grant and the write gate, before the guest's bytes are read.
    if let Err(answered) = permits(caller, Capability::WriteFileBytes) {
        return answered;
    }
    let (_, bytes) = match from_guest(caller, bytes_span) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let state = caller.data_mut();
    if state.current_file().is_none() {
        return refused(NO_FILE);
    }
    let Some(check) = state.check.as_deref() else {
        return refused("no call is running");
    };
    if let Some(why) = check.why_the_file_would_be_invalid(&bytes) {
        return refused(format!("the file was not replaced: {why}"));
    }
    state.replaced = Some(bytes);
    OK
}

/// Keeps the files the guest names together if the gates and the embedder's check agree on all of them, and gives the guest the sentence for a refusal.
fn write_files(
    caller: &mut wasmi::Caller<'_, State>,
    set_span: (i32, i32),
    why: (i32, i32, i32),
) -> Answered {
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    // ⚠ The sentence's slot is checked and cleared before anything is decided.
    if let Err(status) = clear_sentence(memory, caller, why) {
        return bare(status);
    }
    let answered = keep_files(caller, set_span);
    if let (status, Some(sentence)) = &answered {
        if *status != Status::Ok {
            deliver_sentence(memory, caller, why, sentence);
        }
    }
    answered
}

/// Both gates, then the guest's set, then the embedder's check of the whole set; none of it is kept unless all three agree.
fn keep_files(caller: &mut wasmi::Caller<'_, State>, set_span: (i32, i32)) -> Answered {
    if let Err(answered) = permits(caller, Capability::WriteFiles) {
        return answered;
    }
    let (_, bytes) = match from_guest(caller, set_span) {
        Ok(got) => got,
        Err(answered) => return answered,
    };
    let state = caller.data_mut();
    if state.transaction.is_some() {
        return refused(
            "the plugin already wrote its files in this call, and a call writes them in one go",
        );
    }
    let Some(check) = state.check.as_deref() else {
        return refused("no call is running");
    };
    match transaction::admit(&bytes, check) {
        Ok(set) => {
            state.transaction = Some(set);
            OK
        }
        Err((status, sentence)) => (status, Some(sentence)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `match` arm in `link` missing from `SESSION_FUNCTIONS` would be registered with no
    /// capability; a table entry with no arm fails when a guest is opened.
    #[test]
    fn every_session_function_belongs_to_a_capability() {
        let source = include_str!("session.rs");
        for host_fn in SESSION_FUNCTIONS {
            assert!(
                source.contains(&format!("\"{}\" => host_fn!(", host_fn.name)),
                "`{}` is declared in SESSION_FUNCTIONS with no arm in `link`",
                host_fn.name
            );
        }
        for line in source.lines().map(str::trim) {
            let Some(name) = line
                .strip_prefix('"')
                .and_then(|rest| rest.split('"').next())
            else {
                continue;
            };
            if !line.contains("\" => host_fn!(") {
                continue;
            }
            assert!(
                SESSION_FUNCTIONS.iter().any(|f| f.name == name),
                "`{name}` is linked in `link` and absent from SESSION_FUNCTIONS, so it is a host \
                 function no capability pays for"
            );
        }
    }

    #[test]
    fn what_a_grant_buys_a_session_is_derivable() {
        assert!(Session::functions_for(&Grants::none()).is_empty());
        assert!(
            Session::functions_for(&Grants::default()).is_empty(),
            "reading the open document buys a session nothing: it has no described reader"
        );
        assert_eq!(
            Session::functions_for(&Grants::none().granting(Capability::DrawOverlays)),
            ["set_overlay", "overlay_picking"]
        );
        assert_eq!(
            Session::capability_for("draw_widget"),
            Some(Capability::AddPanels)
        );
        assert_eq!(Session::capability_for("patch_field"), None);
    }
}
