//! Plugins that extend a build: a validator reads the plan of a build and reports what is wrong with it, an emitter reads it and adds files.
//!
//! Data in and data out, and nothing else crosses. The plan arrives as the JSON document the
//! command line prints (`openrage-cli wardrobe plan`) without where on the disk each file is copied
//! from, and a validator granted the pack is handed the clothing the build takes as a second document
//! (ABI 1). A finding, with a suggested fix where the validator has one, or a file leaves one call at a
//! time, and each call is checked where it enters the host. A guest has no path, no folder, no
//! document and no clock, and a call runs a fresh instance, so what a rule says is a function of
//! the plan and the pack alone. A plugin extends a build and never hosts one: the plan decides what it does
//! with an emitted file, and the build writes it only when the user builds; a fix writes nothing until a
//! person accepts it.
//!
//! ⚠ As in the runtime, only the functions the grants buy are linked, so an ungranted capability
//! is an unresolved import and not a check one edit could drop.

use crate::abi::Status;
use crate::runtime::{
    explain_link, guest_bytes, memory_of, put_bytes, put_u32, Failure, HostError, Limits,
    HOST_MODULE,
};
use crate::surface::{MAX_NAME, MAX_TEXT};
use crate::transaction::Transaction;
use crate::{Capability, Grants};

/// The export a validator publishes.
pub const VALIDATE: &str = "validate";
/// The export an emitter publishes.
pub const EMIT: &str = "emit";

/// The most bytes of plan a guest is handed; it copies them into its own memory, at its own cost.
pub const MAX_PLAN: usize = 32 << 20;
/// The most findings one call keeps; the rest are refused.
pub const MAX_FINDINGS: usize = 1000;
/// The most files one call keeps.
pub const MAX_FILES: usize = 64;
/// The most text of one file.
pub const MAX_FILE: usize = 1 << 20;
/// The most text of all the files of one call.
pub const MAX_EMITTED: usize = 4 << 20;

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Note,
    Warning,
    /// Stops the build, as a problem of the plan does.
    Error,
}

impl Level {
    /// The code a guest passes to `report`.
    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Note),
            1 => Some(Self::Warning),
            2 => Some(Self::Error),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One thing a validator says is wrong, or worth knowing, about a plan or a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Letters, digits, `_`, `-` and `.`: the plugin's id is put in front of it by whoever shows it.
    pub code: String,
    pub level: Level,
    /// What it is about, such as an item's id or a file's path.
    pub subject: Option<String>,
    pub message: String,
    /// What the validator suggests doing about it, which changes nothing until a person accepts it.
    pub fix: Option<SuggestedFix>,
}

/// A change a validator suggests with a finding: files that are written together if a person accepts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestedFix {
    /// What accepting it does, in a few words a button and a title can say.
    pub label: String,
    pub files: Transaction,
}

/// A text file an emitter adds, at a path inside the resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emitted {
    pub path: String,
    pub text: String,
}

/// What a call produced.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Extended {
    pub findings: Vec<Finding>,
    pub files: Vec<Emitted>,
    /// Every host call that did not answer `Ok`, with the sentence a user is shown where the host has one.
    pub failures: Vec<Failure>,
}

struct HostFn {
    name: &'static str,
    needs: Capability,
}

const FUNCTIONS: &[HostFn] = &[
    HostFn {
        name: "plan_size",
        needs: Capability::ReadPlan,
    },
    HostFn {
        name: "read_plan",
        needs: Capability::ReadPlan,
    },
    HostFn {
        name: "report",
        needs: Capability::ReportFindings,
    },
    HostFn {
        name: "add_file",
        needs: Capability::AddFiles,
    },
    HostFn {
        name: "pack_size",
        needs: Capability::ReadPack,
    },
    HostFn {
        name: "read_pack",
        needs: Capability::ReadPack,
    },
    HostFn {
        name: "report_with_fix",
        needs: Capability::SuggestFixes,
    },
];

/// The host functions `grants` buys a validator or an emitter.
pub fn functions_for(grants: &Grants) -> Vec<&'static str> {
    FUNCTIONS
        .iter()
        .filter(|f| grants.has(f.needs))
        .map(|f| f.name)
        .collect()
}

/// Which capability an import name needs, if a validator or an emitter has such a function.
pub fn capability_for(import: &str) -> Option<Capability> {
    FUNCTIONS.iter().find(|f| f.name == import).map(|f| f.needs)
}

struct State {
    plan: Vec<u8>,
    pack: Vec<u8>,
    findings: Vec<Finding>,
    files: Vec<Emitted>,
    emitted: usize,
    /// The bytes of the files the suggested fixes of this call hold.
    suggested: usize,
    failures: Vec<Failure>,
}

type Answered = (Status, Option<String>);

impl State {
    fn note(&mut self, function: &'static str, (status, because): Answered) -> i32 {
        if status != Status::Ok {
            self.failures.push(Failure {
                function,
                status,
                because,
            });
        }
        status.code()
    }
}

const OK: Answered = (Status::Ok, None);

fn bare(status: Status) -> Answered {
    (status, None)
}

fn refused(why: impl Into<String>) -> Answered {
    (Status::Refused, Some(why.into()))
}

/// Run `export` of `wasm` against `plan`, and take what it reported and added.
///
/// A guest that traps or runs out of fuel produces nothing: what it had reported before is dropped,
/// because a validator that stopped halfway has not said what it would have.
pub fn run(
    wasm: &[u8],
    export: &str,
    grants: &Grants,
    limits: Limits,
    plan: &[u8],
) -> Result<Extended, HostError> {
    run_with_pack(wasm, export, grants, limits, plan, &[])
}

/// [`run`], handing the guest `pack` as well, which a plugin granted `cap.read_pack` reads as data.
pub fn run_with_pack(
    wasm: &[u8],
    export: &str,
    grants: &Grants,
    limits: Limits,
    plan: &[u8],
    pack: &[u8],
) -> Result<Extended, HostError> {
    for (what, held) in [("plan", plan), ("pack", pack)] {
        if held.len() > MAX_PLAN {
            return Err(HostError::Guest(format!(
                "the {what} is {} bytes, and a plugin is handed at most {MAX_PLAN}",
                held.len()
            )));
        }
    }
    let mut config = wasmi::Config::default();
    config.consume_fuel(limits.fuel.is_some());
    let engine = wasmi::Engine::new(&config);
    let module =
        wasmi::Module::new(&engine, wasm).map_err(|e| HostError::NotAModule(e.to_string()))?;
    let mut store = wasmi::Store::new(
        &engine,
        State {
            plan: plan.to_vec(),
            pack: pack.to_vec(),
            findings: Vec::new(),
            files: Vec::new(),
            emitted: 0,
            suggested: 0,
            failures: Vec::new(),
        },
    );
    let mut linker = wasmi::Linker::<State>::new(&engine);
    link(&mut linker, grants)?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| explain_link(&module, &functions_for(grants), capability_for, e))?;
    let func = instance
        .get_typed_func::<(), ()>(&store, export)
        .map_err(|e| HostError::Guest(format!("the plugin has no usable `{export}`: {e}")))?;
    if let Some(fuel) = limits.fuel {
        store
            .set_fuel(fuel)
            .map_err(|e| HostError::Guest(e.to_string()))?;
    }
    func.call(&mut store, ())
        .map_err(|e| HostError::Guest(e.to_string()))?;
    let State {
        findings,
        files,
        failures,
        ..
    } = store.into_data();
    Ok(Extended {
        findings,
        files,
        failures,
    })
}

/// Run a validator's `validate` export.
pub fn validate(
    wasm: &[u8],
    grants: &Grants,
    limits: Limits,
    plan: &[u8],
) -> Result<Extended, HostError> {
    run(wasm, VALIDATE, grants, limits, plan)
}

/// Run a validator's `validate` export with the pack it reads as data.
pub fn validate_with_pack(
    wasm: &[u8],
    grants: &Grants,
    limits: Limits,
    plan: &[u8],
    pack: &[u8],
) -> Result<Extended, HostError> {
    run_with_pack(wasm, VALIDATE, grants, limits, plan, pack)
}

/// Run an emitter's `emit` export.
pub fn emit(
    wasm: &[u8],
    grants: &Grants,
    limits: Limits,
    plan: &[u8],
) -> Result<Extended, HostError> {
    run(wasm, EMIT, grants, limits, plan)
}

fn link(linker: &mut wasmi::Linker<State>, grants: &Grants) -> Result<(), HostError> {
    fn fail(e: impl std::fmt::Display) -> HostError {
        HostError::Guest(e.to_string())
    }
    for host_fn in FUNCTIONS.iter().filter(|f| grants.has(f.needs)) {
        match host_fn.name {
            "plan_size" => linker
                .func_wrap(
                    HOST_MODULE,
                    "plan_size",
                    |mut caller: wasmi::Caller<'_, State>, out: i32| -> i32 {
                        let answered = plan_size(&mut caller, out);
                        caller.data_mut().note("plan_size", answered)
                    },
                )
                .map_err(fail)?,
            "read_plan" => linker
                .func_wrap(
                    HOST_MODULE,
                    "read_plan",
                    |mut caller: wasmi::Caller<'_, State>,
                     offset: i32,
                     out: i32,
                     len: i32|
                     -> i32 {
                        let answered = read_plan(&mut caller, offset, out, len);
                        caller.data_mut().note("read_plan", answered)
                    },
                )
                .map_err(fail)?,
            "report" => linker
                .func_wrap(
                    HOST_MODULE,
                    "report",
                    |mut caller: wasmi::Caller<'_, State>,
                     code: i32,
                     code_len: i32,
                     level: i32,
                     subject: i32,
                     subject_len: i32,
                     message: i32,
                     message_len: i32|
                     -> i32 {
                        let answered = report(
                            &mut caller,
                            (code, code_len),
                            level,
                            (subject, subject_len),
                            (message, message_len),
                        );
                        caller.data_mut().note("report", answered)
                    },
                )
                .map_err(fail)?,
            "add_file" => linker
                .func_wrap(
                    HOST_MODULE,
                    "add_file",
                    |mut caller: wasmi::Caller<'_, State>,
                     path: i32,
                     path_len: i32,
                     text: i32,
                     text_len: i32|
                     -> i32 {
                        let answered = add_file(&mut caller, (path, path_len), (text, text_len));
                        caller.data_mut().note("add_file", answered)
                    },
                )
                .map_err(fail)?,
            "pack_size" => linker
                .func_wrap(
                    HOST_MODULE,
                    "pack_size",
                    |mut caller: wasmi::Caller<'_, State>, out: i32| -> i32 {
                        let answered = held_size(&mut caller, out, |s| s.pack.len());
                        caller.data_mut().note("pack_size", answered)
                    },
                )
                .map_err(fail)?,
            "read_pack" => linker
                .func_wrap(
                    HOST_MODULE,
                    "read_pack",
                    |mut caller: wasmi::Caller<'_, State>,
                     offset: i32,
                     out: i32,
                     len: i32|
                     -> i32 {
                        let answered = held_read(&mut caller, (offset, out, len), |s| &s.pack);
                        caller.data_mut().note("read_pack", answered)
                    },
                )
                .map_err(fail)?,
            "report_with_fix" => linker
                .func_wrap(
                    HOST_MODULE,
                    "report_with_fix",
                    |mut caller: wasmi::Caller<'_, State>,
                     code: i32,
                     code_len: i32,
                     level: i32,
                     subject: i32,
                     subject_len: i32,
                     message: i32,
                     message_len: i32,
                     label: i32,
                     label_len: i32,
                     files: i32,
                     files_len: i32|
                     -> i32 {
                        let answered = report_with_fix(
                            &mut caller,
                            ((code, code_len), level, (subject, subject_len)),
                            (message, message_len),
                            (label, label_len),
                            (files, files_len),
                        );
                        caller.data_mut().note("report_with_fix", answered)
                    },
                )
                .map_err(fail)?,
            other => {
                // Unreachable while FUNCTIONS and this match agree, which
                // `a_function_is_linked_only_for_the_capability_that_buys_it` keeps true.
                return Err(HostError::Guest(format!(
                    "`{other}` is declared in FUNCTIONS and has no implementation"
                )));
            }
        };
    }
    Ok(())
}

fn plan_size(caller: &mut wasmi::Caller<'_, State>, out: i32) -> Answered {
    held_size(caller, out, |s| s.plan.len())
}

fn read_plan(caller: &mut wasmi::Caller<'_, State>, offset: i32, out: i32, len: i32) -> Answered {
    held_read(caller, (offset, out, len), |s| s.plan.as_slice())
}

/// The length of a document the guest is handed, written as a `u32` where it says.
fn held_size(caller: &mut wasmi::Caller<'_, State>, out: i32, of: fn(&State) -> usize) -> Answered {
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    let len = of(caller.data());
    match put_u32(memory, caller, out, len) {
        Ok(()) => OK,
        Err(status) => bare(status),
    }
}

/// `len` bytes of a document the guest is handed, from `offset`, copied to where it says.
fn held_read(
    caller: &mut wasmi::Caller<'_, State>,
    (offset, out, len): (i32, i32, i32),
    of: fn(&State) -> &[u8],
) -> Answered {
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    let (Ok(offset), Ok(len)) = (usize::try_from(offset), usize::try_from(len)) else {
        return bare(Status::BadArgument);
    };
    let Some(end) = offset.checked_add(len) else {
        return bare(Status::BadArgument);
    };
    let Some(chunk) = of(caller.data()).get(offset..end).map(<[u8]>::to_vec) else {
        return bare(Status::BadArgument);
    };
    match put_bytes(memory, caller, out, &chunk) {
        Ok(()) => OK,
        Err(status) => bare(status),
    }
}

/// The text at `span` of the guest's memory, or the answer that says it was not text it owns.
fn text_of(
    caller: &mut wasmi::Caller<'_, State>,
    span: (i32, i32),
    most: usize,
    what: &str,
) -> Result<String, Answered> {
    let Some(memory) = memory_of(caller) else {
        return Err(bare(Status::BadArgument));
    };
    let bytes = guest_bytes(memory, caller, span.0, span.1).map_err(bare)?;
    if bytes.len() > most {
        return Err(refused(format!(
            "{what} is {} bytes, and at most {most} are kept",
            bytes.len()
        )));
    }
    String::from_utf8(bytes).map_err(|_| bare(Status::BadArgument))
}

fn report(
    caller: &mut wasmi::Caller<'_, State>,
    code: (i32, i32),
    level: i32,
    subject: (i32, i32),
    message: (i32, i32),
) -> Answered {
    match finding(caller, (code, level, subject), message) {
        Ok(finding) => keep(caller, finding),
        Err(answered) => answered,
    }
}

/// The finding the guest states, checked where it enters the host.
fn finding(
    caller: &mut wasmi::Caller<'_, State>,
    (code, level, subject): ((i32, i32), i32, (i32, i32)),
    message: (i32, i32),
) -> Result<Finding, Answered> {
    let code = text_of(caller, code, MAX_NAME, "the code")?;
    let subject = text_of(caller, subject, MAX_NAME, "the subject")?;
    let message = text_of(caller, message, MAX_TEXT, "the message")?;
    let Some(level) = Level::from_code(level) else {
        return Err(bare(Status::BadArgument));
    };
    let plain = code
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if code.is_empty() || !plain {
        return Err(refused(format!(
            "the finding's code `{code}` is empty or holds more than letters, digits, `_`, `-` and `.`"
        )));
    }
    if message.trim().is_empty() {
        return Err(refused(
            "the finding's message is empty, so a user would read nothing",
        ));
    }
    Ok(Finding {
        code,
        level,
        subject: Some(subject).filter(|s| !s.is_empty()),
        message,
        fix: None,
    })
}

/// Keep `finding` unless the call already holds as many as it keeps.
fn keep(caller: &mut wasmi::Caller<'_, State>, finding: Finding) -> Answered {
    let state = caller.data_mut();
    if state.findings.len() >= MAX_FINDINGS {
        return refused(format!("a call keeps at most {MAX_FINDINGS} findings"));
    }
    state.findings.push(finding);
    OK
}

/// A finding with the files that put it right, which wait for a person to accept them.
fn report_with_fix(
    caller: &mut wasmi::Caller<'_, State>,
    stated: ((i32, i32), i32, (i32, i32)),
    message: (i32, i32),
    label: (i32, i32),
    files: (i32, i32),
) -> Answered {
    let mut found = match finding(caller, stated, message) {
        Ok(found) => found,
        Err(answered) => return answered,
    };
    let label = match text_of(caller, label, MAX_NAME, "the fix's label") {
        Ok(label) => label,
        Err(answered) => return answered,
    };
    if label.trim().is_empty() {
        return refused("the fix has no label, so a person could not tell what accepting it does");
    }
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    let bytes = match guest_bytes(memory, caller, files.0, files.1) {
        Ok(bytes) => bytes,
        Err(status) => return bare(status),
    };
    let set = match Transaction::decode(&bytes) {
        Ok(set) => set,
        Err(e) => return (e.status(), Some(e.to_string())),
    };
    if let Some(why) = set.problem() {
        return refused(format!("the fix cannot be kept: {why}"));
    }
    let state = caller.data_mut();
    if state.suggested + set.bytes() > MAX_EMITTED {
        return refused(format!(
            "the fixes of one call hold at most {MAX_EMITTED} bytes of files"
        ));
    }
    state.suggested += set.bytes();
    found.fix = Some(SuggestedFix { label, files: set });
    keep(caller, found)
}

fn add_file(caller: &mut wasmi::Caller<'_, State>, path: (i32, i32), text: (i32, i32)) -> Answered {
    let path = match text_of(caller, path, MAX_NAME, "the path") {
        Ok(path) => path,
        Err(answered) => return answered,
    };
    let text = match text_of(caller, text, MAX_FILE, "the file") {
        Ok(text) => text,
        Err(answered) => return answered,
    };
    if path.is_empty() || path.contains('\0') {
        return refused("the file's path is empty or holds a NUL");
    }
    let state = caller.data_mut();
    if state.files.len() >= MAX_FILES {
        return refused(format!("a call keeps at most {MAX_FILES} files"));
    }
    if state.emitted + text.len() > MAX_EMITTED {
        return refused(format!(
            "the files of one call hold at most {MAX_EMITTED} bytes of text"
        ));
    }
    state.emitted += text.len();
    state.files.push(Emitted { path, text });
    OK
}
