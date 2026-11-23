//! Running a guest, and surviving it.
//!
//! A guest trap (`unreachable`, which is what a Rust panic compiles to) arrives
//! as `Err` with the host intact, and a guest that spins is stopped by
//! [`Limits::fuel`] — measured at ~9 ms for an unbounded `loop br 0`.
//!
//! ⚠ [`Runtime::run`] registers only what the [`Grants`] permit, so an
//! ungranted capability has no import to resolve and the guest fails to
//! instantiate; a check inside each host function would be one edit from being
//! dropped. [`Runtime::explain`] only names the capability after a link failure.
//!
//! This file marshals and does not decide: what a plugin may see and change is
//! [`crate::host`], which knows nothing about WebAssembly.

use std::borrow::Cow;

use or_io::desc::{Image, Pack, Value};

use crate::abi::{self, Status};
use crate::host::{self, Denied, SaveCheck};
use crate::transaction::{self, Transaction};
use crate::{Because, Capability, Grants, Permissions, Refused, WriteGate};

/// What a guest is allowed to consume.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Instructions, roughly. `None` lets a guest run forever.
    ///
    /// ⚠ `None` is for a headless batch tool. The application must not pass it:
    /// an unbounded guest is a frozen editor, which WASM does not prevent on its
    /// own.
    pub fuel: Option<u64>,
    /// Linear memory pages the guest may grow to, 64 KiB each.
    pub memory_pages: u32,
}

impl Default for Limits {
    /// 10 million instructions stops a spin in milliseconds; 256 pages is 16 MiB.
    fn default() -> Self {
        Self {
            fuel: Some(10_000_000),
            memory_pages: 256,
        }
    }
}

/// Why a guest could not be run.
#[derive(Debug)]
pub enum HostError {
    /// The bytes are not a module this runtime accepts.
    NotAModule(String),
    /// The guest imports something the grants do not provide; `needs` names the
    /// capability that would, so a host can offer the grant.
    Unresolved {
        import: String,
        needs: Option<Capability>,
    },
    /// The guest trapped, hit its fuel limit, or otherwise failed while running;
    /// the host is intact.
    Guest(String),
    /// The guest failed after a capability was refused, which is the likelier
    /// cause: a guest that stops on a refusal otherwise reads as merely broken.
    StoppedAfterRefusal { why: String, refused: Vec<Refused> },
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAModule(why) => write!(f, "not a WebAssembly module: {why}"),
            Self::Unresolved { import, needs } => match needs {
                Some(capability) => write!(
                    f,
                    "the plugin imports `{import}`, which needs permission to {}",
                    capability.asks_for()
                ),
                None => write!(
                    f,
                    "the plugin imports `{import}`, which this host does not provide"
                ),
            },
            Self::Guest(why) => write!(f, "the plugin failed while running: {why}"),
            Self::StoppedAfterRefusal { why, refused } => {
                write!(
                    f,
                    "the plugin failed while running ({why}) after a refusal: "
                )?;
                for (i, refusal) in refused.iter().enumerate() {
                    if i > 0 {
                        f.write_str("; ")?;
                    }
                    write!(f, "{refusal}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for HostError {}

/// The module namespace every host function lives in.
pub const HOST_MODULE: &str = "openrage";

/// The export a guest must publish for anything but an integer to cross.
///
/// ⚠ Exported by the guest, never imported: the guest owns every buffer the
/// host writes into ([`crate::abi`]), and a host-supplied memory would be an
/// address space the guest's allocator knows nothing about.
pub const GUEST_MEMORY: &str = "memory";

/// One host function, and the capability that buys it; a function without a
/// capability cannot be expressed.
struct HostFn {
    name: &'static str,
    needs: Capability,
}

/// Every host function this ABI defines, each with the capability it needs.
///
/// The seven described readers are enough to walk a document: a guest indexes
/// a run itself from `follow`, `element_count` and `type_size`, so the host
/// never builds a tree; see [`crate::host`]. The file's bytes are three more.
const HOST_FUNCTIONS: &[HostFn] = &[
    HostFn {
        name: "root_type",
        needs: Capability::ReadOpenDocument,
    },
    HostFn {
        name: "field_count",
        needs: Capability::ReadOpenDocument,
    },
    HostFn {
        name: "field_name",
        needs: Capability::ReadOpenDocument,
    },
    HostFn {
        name: "type_size",
        needs: Capability::ReadOpenDocument,
    },
    HostFn {
        name: "field_value",
        needs: Capability::ReadOpenDocument,
    },
    HostFn {
        name: "follow",
        needs: Capability::ReadOpenDocument,
    },
    HostFn {
        name: "element_count",
        needs: Capability::ReadOpenDocument,
    },
    HostFn {
        name: "patch_field",
        needs: Capability::EditOpenDocument,
    },
    HostFn {
        name: "file_size",
        needs: Capability::ReadFileBytes,
    },
    HostFn {
        name: "read_file",
        needs: Capability::ReadFileBytes,
    },
    HostFn {
        name: "write_file",
        needs: Capability::WriteFileBytes,
    },
    HostFn {
        name: "write_files",
        needs: Capability::WriteFiles,
    },
];

/// The document a plugin was opened on; a `ReadOnly` one refuses every patch,
/// whatever the grants.
pub enum Document<'d> {
    ReadOnly(&'d [u8]),
    Writable(&'d mut [u8]),
}

impl Document<'_> {
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::ReadOnly(bytes) => bytes,
            Self::Writable(bytes) => bytes,
        }
    }

    fn writable(&mut self) -> Option<&mut [u8]> {
        match self {
            Self::ReadOnly(_) => None,
            Self::Writable(bytes) => Some(bytes),
        }
    }
}

/// What the guest is run against: a pack, a root type name and the document's
/// bytes. A typed value such as `&GtaDrawable` cannot cross into a guest.
pub struct Subject<'p> {
    pub pack: &'p Pack,
    pub root_type: String,
    /// The resource payload, with the root type at offset zero.
    pub document: Document<'p>,
    /// Where the virtual half of the payload ends, for resolving a pointer.
    pub virtual_size: usize,
    /// The file the document would save, which `read_file` serves and `write_file` replaces; `None` holds no file.
    pub file: Option<&'p [u8]>,
}

impl<'p> Subject<'p> {
    pub fn reading(
        pack: &'p Pack,
        root_type: impl Into<String>,
        document: &'p [u8],
        virtual_size: usize,
    ) -> Self {
        Self {
            pack,
            root_type: root_type.into(),
            document: Document::ReadOnly(document),
            virtual_size,
            file: None,
        }
    }

    pub fn editing(
        pack: &'p Pack,
        root_type: impl Into<String>,
        document: &'p mut [u8],
        virtual_size: usize,
    ) -> Self {
        Self {
            pack,
            root_type: root_type.into(),
            document: Document::Writable(document),
            virtual_size,
            file: None,
        }
    }

    /// A subject with no document behind it; reading a field of one answers
    /// [`Status::Truncated`], as a pack disagreeing with a real file does.
    pub fn describing(pack: &'p Pack, root_type: impl Into<String>) -> Self {
        Self::reading(pack, root_type, &[], 0)
    }

    /// The same subject with `file` as the bytes of the file its document would save.
    pub fn with_file(mut self, file: &'p [u8]) -> Self {
        self.file = Some(file);
        self
    }

    fn image(&self) -> Image<'_> {
        Image::new(self.document.bytes(), self.virtual_size)
    }
}

/// Host state a guest's calls act on.
struct State<'p, G: WriteGate> {
    subject: Subject<'p>,
    permissions: Permissions<G>,
    check: &'p dyn SaveCheck,
    /// Set by a patch the three gates kept.
    touched: bool,
    /// The file `read_file` serves: the subject's own, until a write the gates kept makes it the guest's.
    file: Option<Cow<'p, [u8]>>,
    /// The files `write_files` kept, for the embedder to apply together when the run ends.
    transaction: Option<Transaction>,
    failures: Vec<Failure>,
}

impl<G: WriteGate> State<'_, G> {
    /// Whether a write the gates kept has replaced the file.
    fn replaced(&self) -> bool {
        matches!(self.file, Some(Cow::Owned(_)))
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
}

/// A host call that answered with something other than [`Status::Ok`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub function: &'static str,
    pub status: Status,
    /// What the host would tell a user, where it knows more than the status.
    pub because: Option<String>,
}

/// A configured host.
pub struct Runtime {
    limits: Limits,
}

/// What a run produced.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Whether a patch or a file the gates kept changed the document.
    pub touched: bool,
    /// The file the guest wrote last and the gates kept, for the embedder to put in place of its document.
    pub replaced: Option<Vec<u8>>,
    /// The files the guest wrote together and the gates and the embedder's check kept, all of them or none, for the embedder to apply as one step.
    pub transaction: Option<Transaction>,
    /// Capability refusals the guest collected, in order, each carrying the sentence a user is shown.
    pub refused: Vec<Refused>,
    /// Every host call that did not answer `Ok`.
    ///
    /// ⚠ Includes [`Status::BufferTooSmall`] (the first half of a two-call read)
    /// and [`Status::NullPointer`] (an absent child), neither a fault: drop them
    /// before reporting to a user, keep them when diagnosing a guest that seems
    /// to do nothing, which is what ignoring a retry looks like.
    pub failures: Vec<Failure>,
}

impl Runtime {
    pub fn new(limits: Limits) -> Self {
        Self { limits }
    }

    /// The host functions `grants` buys, for a consent prompt or a log.
    pub fn functions_for(grants: &Grants) -> Vec<&'static str> {
        HOST_FUNCTIONS
            .iter()
            .filter(|f| grants.has(f.needs))
            .map(|f| f.name)
            .collect()
    }

    /// Which capability an import name needs, if this host defines it.
    pub fn capability_for(import: &str) -> Option<Capability> {
        HOST_FUNCTIONS
            .iter()
            .find(|f| f.name == import)
            .map(|f| f.needs)
    }

    /// Name the capability behind a link failure, or pass the failure through
    /// unchanged when no import explains it. Explains, never enforces.
    fn explain(module: &wasmi::Module, grants: &Grants, error: wasmi::Error) -> HostError {
        explain_link(
            module,
            &Self::functions_for(grants),
            Self::capability_for,
            error,
        )
    }

    /// Run `entry` in `wasm`, with only what `grants` permits.
    ///
    /// A guest trap returns `Err` and leaves the host intact; a host panic is not
    /// caught, because guest code never runs as host code. `check` has no default, so an embedder cannot get a writing host
    /// by forgetting one — see [`SaveCheck`].
    pub fn run<G: WriteGate>(
        &self,
        wasm: &[u8],
        entry: &str,
        subject: Subject<'_>,
        grants: Grants,
        gate: G,
        check: &dyn SaveCheck,
    ) -> Result<Outcome, HostError> {
        let mut config = wasmi::Config::default();
        config.consume_fuel(self.limits.fuel.is_some());
        let engine = wasmi::Engine::new(&config);

        let module =
            wasmi::Module::new(&engine, wasm).map_err(|e| HostError::NotAModule(e.to_string()))?;

        let mut store = wasmi::Store::new(
            &engine,
            State {
                file: subject.file.map(Cow::Borrowed),
                subject,
                permissions: Permissions::new(grants.clone(), gate),
                check,
                touched: false,
                transaction: None,
                failures: Vec::new(),
            },
        );
        if let Some(fuel) = self.limits.fuel {
            store
                .set_fuel(fuel)
                .map_err(|e| HostError::Guest(e.to_string()))?;
        }

        let mut linker = wasmi::Linker::<State<'_, G>>::new(&engine);

        // ⚠ Only the functions the grants buy are defined, so an ungranted
        // capability is an unresolved import rather than a runtime check.
        for host_fn in HOST_FUNCTIONS.iter().filter(|f| grants.has(f.needs)) {
            match host_fn.name {
                "root_type" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "root_type",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             out_ptr: i32,
                             out_cap: i32,
                             out_len_ptr: i32|
                             -> i32 {
                                let (status, why) =
                                    root_type(&mut caller, out_ptr, out_cap, out_len_ptr);
                                caller.data_mut().note("root_type", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "field_count" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "field_count",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             type_ptr: i32,
                             type_len: i32,
                             out_ptr: i32|
                             -> i32 {
                                let (status, why) =
                                    field_count(&mut caller, type_ptr, type_len, out_ptr);
                                caller.data_mut().note("field_count", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "field_name" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "field_name",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             type_ptr: i32,
                             type_len: i32,
                             index: i32,
                             out_ptr: i32,
                             out_cap: i32,
                             out_len_ptr: i32|
                             -> i32 {
                                let (status, why) = field_name(
                                    &mut caller,
                                    type_ptr,
                                    type_len,
                                    index,
                                    out_ptr,
                                    out_cap,
                                    out_len_ptr,
                                );
                                caller.data_mut().note("field_name", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "type_size" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "type_size",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             type_ptr: i32,
                             type_len: i32,
                             out_ptr: i32|
                             -> i32 {
                                let (status, why) =
                                    type_size(&mut caller, type_ptr, type_len, out_ptr);
                                caller.data_mut().note("type_size", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "field_value" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "field_value",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             type_ptr: i32,
                             type_len: i32,
                             at: i32,
                             name_ptr: i32,
                             name_len: i32,
                             out_ptr: i32,
                             out_cap: i32,
                             out_len_ptr: i32|
                             -> i32 {
                                let (status, why) = field_value(
                                    &mut caller,
                                    (type_ptr, type_len),
                                    at,
                                    (name_ptr, name_len),
                                    (out_ptr, out_cap, out_len_ptr),
                                );
                                caller.data_mut().note("field_value", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "follow" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "follow",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             type_ptr: i32,
                             type_len: i32,
                             at: i32,
                             name_ptr: i32,
                             name_len: i32,
                             out_ptr: i32|
                             -> i32 {
                                let (status, why) = about_a_field(
                                    &mut caller,
                                    (type_ptr, type_len),
                                    at,
                                    (name_ptr, name_len),
                                    out_ptr,
                                    host::follow,
                                );
                                caller.data_mut().note("follow", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "element_count" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "element_count",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             type_ptr: i32,
                             type_len: i32,
                             at: i32,
                             name_ptr: i32,
                             name_len: i32,
                             out_ptr: i32|
                             -> i32 {
                                let (status, why) = about_a_field(
                                    &mut caller,
                                    (type_ptr, type_len),
                                    at,
                                    (name_ptr, name_len),
                                    out_ptr,
                                    host::element_count,
                                );
                                caller.data_mut().note("element_count", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "patch_field" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "patch_field",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             type_ptr: i32,
                             type_len: i32,
                             at: i32,
                             name_ptr: i32,
                             name_len: i32,
                             value_ptr: i32,
                             value_len: i32|
                             -> i32 {
                                let (status, why) = patch_field(
                                    &mut caller,
                                    (type_ptr, type_len),
                                    at,
                                    (name_ptr, name_len),
                                    (value_ptr, value_len),
                                );
                                caller.data_mut().note("patch_field", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "file_size" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "file_size",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>, out_ptr: i32| -> i32 {
                                let (status, why) = file_size(&mut caller, out_ptr);
                                caller.data_mut().note("file_size", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "read_file" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "read_file",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             offset: i32,
                             out_ptr: i32,
                             len: i32|
                             -> i32 {
                                let (status, why) = read_file(&mut caller, offset, out_ptr, len);
                                caller.data_mut().note("read_file", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "write_file" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "write_file",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             bytes_ptr: i32,
                             bytes_len: i32,
                             why_ptr: i32,
                             why_cap: i32,
                             why_len_ptr: i32|
                             -> i32 {
                                let (status, why) = write_file(
                                    &mut caller,
                                    (bytes_ptr, bytes_len),
                                    (why_ptr, why_cap, why_len_ptr),
                                );
                                caller.data_mut().note("write_file", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                "write_files" => {
                    linker
                        .func_wrap(
                            HOST_MODULE,
                            "write_files",
                            |mut caller: wasmi::Caller<'_, State<'_, G>>,
                             set_ptr: i32,
                             set_len: i32,
                             why_ptr: i32,
                             why_cap: i32,
                             why_len_ptr: i32|
                             -> i32 {
                                let (status, why) = write_files(
                                    &mut caller,
                                    (set_ptr, set_len),
                                    (why_ptr, why_cap, why_len_ptr),
                                );
                                caller.data_mut().note("write_files", status, why);
                                status.code()
                            },
                        )
                        .map_err(|e| HostError::Guest(e.to_string()))?;
                }
                other => {
                    // Unreachable while `HOST_FUNCTIONS` and this match agree, and
                    // `every_registered_function_belongs_to_a_capability` is what
                    // keeps them agreeing.
                    return Err(HostError::Guest(format!(
                        "`{other}` is declared in HOST_FUNCTIONS and has no implementation"
                    )));
                }
            }
        }

        // ⚠ The linker is the enforcement; `explain` only names what it refused.
        // A pre-check of the imports against the grants was refuted: with it in
        // place, removing the `grants.has(..)` filter above left every test
        // passing. Without it, removing the filter lets the guest reach what it
        // was not granted and `an_ungranted_capability_has_no_function_to_call`
        // fails.
        let instance = match linker.instantiate_and_start(&mut store, &module) {
            Ok(instance) => instance,
            Err(e) => return Err(Self::explain(&module, &grants, e)),
        };
        let func = instance
            .get_typed_func::<(), ()>(&store, entry)
            .map_err(|e| HostError::Guest(e.to_string()))?;

        // A guest trap, fuel exhaustion and stack overflow all arrive here as
        // `Err`, with the host intact.
        let ran = func.call(&mut store, ());

        let State {
            permissions,
            touched,
            file,
            transaction,
            failures,
            ..
        } = store.into_data();
        let refused = permissions.refusals().to_vec();
        let replaced = match file {
            Some(Cow::Owned(bytes)) => Some(bytes),
            _ => None,
        };

        match ran {
            Ok(()) => Ok(Outcome {
                touched: touched || replaced.is_some(),
                replaced,
                transaction,
                refused,
                failures,
            }),
            Err(e) if refused.is_empty() => Err(HostError::Guest(e.to_string())),
            Err(e) => Err(HostError::StoppedAfterRefusal {
                why: e.to_string(),
                refused,
            }),
        }
    }
}

/// Name the capability behind a link failure: the first import the host does not list in `provided`,
/// with the capability `needs` says buys it.
pub(crate) fn explain_link(
    module: &wasmi::Module,
    provided: &[&str],
    needs: impl Fn(&str) -> Option<Capability>,
    error: wasmi::Error,
) -> HostError {
    for import in module.imports() {
        if import.module() != HOST_MODULE {
            return HostError::Unresolved {
                import: format!("{}::{}", import.module(), import.name()),
                needs: None,
            };
        }
        if !provided.contains(&import.name()) {
            return HostError::Unresolved {
                import: import.name().to_string(),
                needs: needs(import.name()),
            };
        }
    }
    HostError::Guest(error.to_string())
}

/// The guest's exported memory, or `None` if it published none.
pub(crate) fn memory_of<T>(caller: &mut wasmi::Caller<'_, T>) -> Option<wasmi::Memory> {
    caller
        .get_export(GUEST_MEMORY)
        .and_then(wasmi::Extern::into_memory)
}

pub(crate) fn guest_bytes<T>(
    memory: wasmi::Memory,
    caller: &wasmi::Caller<'_, T>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, Status> {
    let (Ok(ptr), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return Err(Status::BadArgument);
    };
    let end = ptr.checked_add(len).ok_or(Status::BadArgument)?;
    memory
        .data(caller)
        .get(ptr..end)
        .map(<[u8]>::to_vec)
        .ok_or(Status::BadArgument)
}

fn guest_str<T>(
    memory: wasmi::Memory,
    caller: &wasmi::Caller<'_, T>,
    span: (i32, i32),
) -> Result<String, Status> {
    String::from_utf8(guest_bytes(memory, caller, span.0, span.1)?).map_err(|_| Status::BadArgument)
}

pub(crate) fn put_bytes<T>(
    memory: wasmi::Memory,
    caller: &mut wasmi::Caller<'_, T>,
    ptr: i32,
    bytes: &[u8],
) -> Result<(), Status> {
    let Ok(ptr) = usize::try_from(ptr) else {
        return Err(Status::BadArgument);
    };
    let end = ptr.checked_add(bytes.len()).ok_or(Status::BadArgument)?;
    let data = memory.data_mut(caller);
    data.get_mut(ptr..end)
        .ok_or(Status::BadArgument)?
        .copy_from_slice(bytes);
    Ok(())
}

pub(crate) fn put_u32<T>(
    memory: wasmi::Memory,
    caller: &mut wasmi::Caller<'_, T>,
    ptr: i32,
    value: usize,
) -> Result<(), Status> {
    let value = u32::try_from(value).map_err(|_| Status::TooLarge)?;
    put_bytes(memory, caller, ptr, &value.to_le_bytes())
}

/// Hand `answer` to the guest under the `(ptr, cap, len_ptr)` convention.
///
/// ⚠ **The length slot is written whether or not the answer fits**, because it
/// is the only way a guest learns what to retry with. The *buffer* is written
/// only when the whole answer fits: a short buffer receives nothing, so a
/// decoder that ignored the status reads what was already there rather than
/// half a tree.
pub(crate) fn deliver<T>(
    memory: wasmi::Memory,
    caller: &mut wasmi::Caller<'_, T>,
    out: (i32, i32, i32),
    answer: &[u8],
) -> Status {
    let (out_ptr, out_cap, out_len_ptr) = out;
    let Ok(cap) = usize::try_from(out_cap) else {
        return Status::BadArgument;
    };
    if let Err(status) = put_u32(memory, caller, out_len_ptr, answer.len()) {
        return status;
    }
    if answer.len() > cap {
        return Status::BufferTooSmall;
    }
    match put_bytes(memory, caller, out_ptr, answer) {
        Ok(()) => Status::Ok,
        Err(status) => status,
    }
}

/// The status a capability refusal answers with; the two reasons stay two
/// statuses because their remedies differ.
pub(crate) fn refusal_status(because: &Because) -> Status {
    match because {
        Because::NotGranted => Status::NotGranted,
        Because::EmbedderRefusesWrites(_) => Status::NotWritable,
    }
}

type Answered = (Status, Option<String>);

const OK: Answered = (Status::Ok, None);

fn bare(status: Status) -> Answered {
    (status, None)
}

fn denied(denied: Denied) -> Answered {
    (denied.status, Some(denied.because))
}

fn permits<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    capability: Capability,
) -> Result<(), Answered> {
    caller
        .data_mut()
        .permissions
        .permits(capability)
        .map_err(|refused| (refusal_status(&refused.because), Some(refused.to_string())))
}

/// What every reading arm does first: the read permission, then the guest's
/// memory.
macro_rules! ready {
    ($caller:expr) => {{
        if let Err(answered) = permits($caller, Capability::ReadOpenDocument) {
            return answered;
        }
        match memory_of($caller) {
            Some(memory) => memory,
            None => return bare(Status::BadArgument),
        }
    }};
}

fn root_type<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    out_ptr: i32,
    out_cap: i32,
    out_len_ptr: i32,
) -> Answered {
    let memory = ready!(caller);
    let name = caller.data().subject.root_type.clone();
    bare(deliver(
        memory,
        caller,
        (out_ptr, out_cap, out_len_ptr),
        name.as_bytes(),
    ))
}

fn field_count<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    type_ptr: i32,
    type_len: i32,
    out_ptr: i32,
) -> Answered {
    let memory = ready!(caller);
    let type_name = match guest_str(memory, caller, (type_ptr, type_len)) {
        Ok(name) => name,
        Err(status) => return bare(status),
    };
    let count = match host::field_count(caller.data().subject.pack, &type_name) {
        Ok(count) => count,
        Err(why) => return denied(why),
    };
    match put_u32(memory, caller, out_ptr, count) {
        Ok(()) => OK,
        Err(status) => bare(status),
    }
}

fn type_size<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    type_ptr: i32,
    type_len: i32,
    out_ptr: i32,
) -> Answered {
    let memory = ready!(caller);
    let type_name = match guest_str(memory, caller, (type_ptr, type_len)) {
        Ok(name) => name,
        Err(status) => return bare(status),
    };
    let size = match host::type_size(caller.data().subject.pack, &type_name) {
        Ok(size) => size,
        Err(why) => return denied(why),
    };
    match put_u32(memory, caller, out_ptr, size) {
        Ok(()) => OK,
        Err(status) => bare(status),
    }
}

fn field_name<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    type_ptr: i32,
    type_len: i32,
    index: i32,
    out_ptr: i32,
    out_cap: i32,
    out_len_ptr: i32,
) -> Answered {
    let memory = ready!(caller);
    let type_name = match guest_str(memory, caller, (type_ptr, type_len)) {
        Ok(name) => name,
        Err(status) => return bare(status),
    };
    let Ok(index) = usize::try_from(index) else {
        return bare(Status::BadArgument);
    };
    let name = match host::field_name(caller.data().subject.pack, &type_name, index) {
        Ok(name) => name.to_string(),
        Err(why) => return denied(why),
    };
    bare(deliver(
        memory,
        caller,
        (out_ptr, out_cap, out_len_ptr),
        name.as_bytes(),
    ))
}

fn field_value<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    type_span: (i32, i32),
    at: i32,
    name_span: (i32, i32),
    out: (i32, i32, i32),
) -> Answered {
    let memory = ready!(caller);
    let (type_name, field, at) = match arguments(memory, caller, type_span, at, name_span) {
        Ok(parts) => parts,
        Err(status) => return bare(status),
    };
    let encoded = {
        let subject = &caller.data().subject;
        match host::field_value(
            subject.pack,
            subject.image(),
            host::Site::new(&type_name, at, &field),
        ) {
            Ok(value) => match abi::encode(&value) {
                Ok(bytes) => bytes,
                Err(e) => return (e.status(), Some(e.to_string())),
            },
            Err(why) => return denied(why),
        }
    };
    bare(deliver(memory, caller, out, &encoded))
}

/// `follow` and `element_count`: same arguments, same shape of answer.
fn about_a_field<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    type_span: (i32, i32),
    at: i32,
    name_span: (i32, i32),
    out_ptr: i32,
    answer: fn(&Pack, Image<'_>, host::Site<'_>) -> Result<usize, Denied>,
) -> Answered {
    let memory = ready!(caller);
    let (type_name, field, at) = match arguments(memory, caller, type_span, at, name_span) {
        Ok(parts) => parts,
        Err(status) => return bare(status),
    };
    let value = {
        let subject = &caller.data().subject;
        match answer(
            subject.pack,
            subject.image(),
            host::Site::new(&type_name, at, &field),
        ) {
            Ok(value) => value,
            Err(why) => return denied(why),
        }
    };
    match put_u32(memory, caller, out_ptr, value) {
        Ok(()) => OK,
        Err(status) => bare(status),
    }
}

fn patch_field<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    type_span: (i32, i32),
    at: i32,
    name_span: (i32, i32),
    value_span: (i32, i32),
) -> Answered {
    // Both the grant and the write gate, before anything is read.
    if let Err(answered) = permits(caller, Capability::EditOpenDocument) {
        return answered;
    }
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    let (type_name, field, at) = match arguments(memory, caller, type_span, at, name_span) {
        Ok(parts) => parts,
        Err(status) => return bare(status),
    };
    let encoded = match guest_bytes(memory, caller, value_span.0, value_span.1) {
        Ok(bytes) => bytes,
        Err(status) => return bare(status),
    };
    // With the pack, so a struct is refused below as not a scalar rather than
    // here as unreadable.
    let value = match abi::decode_with(&encoded, Some(caller.data().subject.pack)) {
        Ok(value) => value,
        Err(e) => return (e.status(), Some(e.to_string())),
    };
    let scalar = match value {
        Value::Scalar(scalar) => scalar,
        // An enum arrives as its variant's value; `or_io::write::patch` refuses
        // one the enum does not declare.
        Value::Enum { value, .. } => or_io::desc::value::ScalarValue::Int(i128::from(value)),
        other => {
            return (
                Status::Refused,
                Some(format!(
                    "a patch takes a scalar and this is {}; anything else moves something",
                    describe(&other)
                )),
            )
        }
    };

    let state = caller.data_mut();
    if state.replaced() {
        return (
            Status::Refused,
            Some(
                "the plugin replaced the file in this run, so a patch would land in the \
                 document as it was before and be lost"
                    .into(),
            ),
        );
    }
    let pack = state.subject.pack;
    let virtual_size = state.subject.virtual_size;
    let check = state.check;
    let Some(document) = state.subject.document.writable() else {
        return (
            Status::NotWritable,
            Some("the plugin was opened on a document its host holds read-only".into()),
        );
    };

    match host::patch_field(
        pack,
        document,
        virtual_size,
        host::Site::new(&type_name, at, &field),
        scalar,
        check,
    ) {
        Ok(()) => {
            caller.data_mut().touched = true;
            OK
        }
        Err(why) => denied(why),
    }
}

pub(crate) const NO_FILE: &str =
    "the host holds no file for this document, so there are no bytes to read or replace";

fn file_size<G: WriteGate>(caller: &mut wasmi::Caller<'_, State<'_, G>>, out_ptr: i32) -> Answered {
    if let Err(answered) = permits(caller, Capability::ReadFileBytes) {
        return answered;
    }
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    let Some(len) = caller.data().file.as_deref().map(<[u8]>::len) else {
        return (Status::Refused, Some(NO_FILE.into()));
    };
    match put_u32(memory, caller, out_ptr, len) {
        Ok(()) => OK,
        Err(status) => bare(status),
    }
}

/// Copies from the bytes the embedder already holds into the guest's memory: nothing is built on the host's side.
fn read_file<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
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
    let Some(file) = state.file.as_deref() else {
        return (Status::Refused, Some(NO_FILE.into()));
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
fn write_file<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    bytes_span: (i32, i32),
    why: (i32, i32, i32),
) -> Answered {
    let Some(memory) = memory_of(caller) else {
        return bare(Status::BadArgument);
    };
    // ⚠ The sentence's slot is checked and cleared before anything is decided,
    // so a bad slot is never the answer to a write that was kept.
    if let Err(status) = clear_sentence(memory, caller, why) {
        return bare(status);
    }
    let answered = replace_file(caller, memory, bytes_span);
    if let (status, Some(sentence)) = &answered {
        if *status != Status::Ok {
            deliver_sentence(memory, caller, why, sentence);
        }
    }
    answered
}

/// Both gates, then the guest's bytes, then the embedder's check of the file; the bytes are kept only once all three agree.
fn replace_file<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    memory: wasmi::Memory,
    bytes_span: (i32, i32),
) -> Answered {
    // Both the grant and the write gate, before the guest's bytes are read.
    if let Err(answered) = permits(caller, Capability::WriteFileBytes) {
        return answered;
    }
    let bytes = match guest_bytes(memory, caller, bytes_span.0, bytes_span.1) {
        Ok(bytes) => bytes,
        Err(status) => return bare(status),
    };
    let state = caller.data_mut();
    if state.subject.document.writable().is_none() {
        return (
            Status::NotWritable,
            Some("the plugin was opened on a document its host holds read-only".into()),
        );
    }
    if state.file.is_none() {
        return (Status::Refused, Some(NO_FILE.into()));
    }
    if state.touched {
        return (
            Status::Refused,
            Some(
                "the plugin patched a field in this run, and replacing the file would \
                 drop the patch"
                    .into(),
            ),
        );
    }
    if let Some(why) = state.check.why_the_file_would_be_invalid(&bytes) {
        return (
            Status::Refused,
            Some(format!("the file was not replaced: {why}")),
        );
    }
    state.file = Some(Cow::Owned(bytes));
    OK
}

/// Keeps the files the guest names together if the gates and the embedder's check agree on all of them, and gives the guest the sentence for a refusal.
fn write_files<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
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
    let answered = keep_files(caller, memory, set_span);
    if let (status, Some(sentence)) = &answered {
        if *status != Status::Ok {
            deliver_sentence(memory, caller, why, sentence);
        }
    }
    answered
}

/// Both gates, then the guest's set, then the embedder's check of the whole set; none of it is kept unless all three agree.
fn keep_files<G: WriteGate>(
    caller: &mut wasmi::Caller<'_, State<'_, G>>,
    memory: wasmi::Memory,
    set_span: (i32, i32),
) -> Answered {
    // Both the grant and the write gate, before the guest's bytes are read.
    if let Err(answered) = permits(caller, Capability::WriteFiles) {
        return answered;
    }
    let bytes = match guest_bytes(memory, caller, set_span.0, set_span.1) {
        Ok(bytes) => bytes,
        Err(status) => return bare(status),
    };
    let state = caller.data_mut();
    if state.transaction.is_some() {
        return (
            Status::Refused,
            Some(
                "the plugin already wrote its files in this run, and a run writes them in one call"
                    .into(),
            ),
        );
    }
    match transaction::admit(&bytes, state.check) {
        Ok(set) => {
            state.transaction = Some(set);
            OK
        }
        Err((status, sentence)) => (status, Some(sentence)),
    }
}

/// Checks the `(ptr, cap, len_ptr)` slot a sentence is delivered into and writes a length of zero there.
pub(crate) fn clear_sentence<T>(
    memory: wasmi::Memory,
    caller: &mut wasmi::Caller<'_, T>,
    (ptr, cap, len_ptr): (i32, i32, i32),
) -> Result<(), Status> {
    let (Ok(ptr), Ok(cap)) = (usize::try_from(ptr), usize::try_from(cap)) else {
        return Err(Status::BadArgument);
    };
    let end = ptr.checked_add(cap).ok_or(Status::BadArgument)?;
    if end > memory.data(&*caller).len() {
        return Err(Status::BadArgument);
    }
    put_u32(memory, caller, len_ptr, 0)
}

/// Writes `sentence`'s length, always, and as much of it as fits, cut at a character, into a slot [`clear_sentence`] checked.
pub(crate) fn deliver_sentence<T>(
    memory: wasmi::Memory,
    caller: &mut wasmi::Caller<'_, T>,
    (ptr, cap, len_ptr): (i32, i32, i32),
    sentence: &str,
) {
    let mut cut = sentence.len().min(usize::try_from(cap).unwrap_or(0));
    while !sentence.is_char_boundary(cut) {
        cut -= 1;
    }
    // Neither can fail: the slot was checked, and a sentence is not 4 GiB.
    let _ = put_u32(memory, caller, len_ptr, sentence.len());
    let _ = put_bytes(memory, caller, ptr, &sentence.as_bytes()[..cut]);
}

/// The type name, the field name and the offset, read out of the guest.
fn arguments<G: WriteGate>(
    memory: wasmi::Memory,
    caller: &wasmi::Caller<'_, State<'_, G>>,
    type_span: (i32, i32),
    at: i32,
    name_span: (i32, i32),
) -> Result<(String, String, usize), Status> {
    let type_name = guest_str(memory, caller, type_span)?;
    let field = guest_str(memory, caller, name_span)?;
    let at = usize::try_from(at).map_err(|_| Status::BadArgument)?;
    Ok((type_name, field, at))
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Scalar(_) => "a scalar",
        Value::Enum { .. } => "an enum",
        Value::Struct(_) => "a struct",
        Value::Blob { .. } => "a codec's own value",
        Value::Array { .. } | Value::Records(_) => "an array",
        Value::Bytes(_) | Value::Opaque(_) => "a run of bytes",
        Value::Ptr { .. } | Value::Alias(_) | Value::Member { .. } | Value::UndescribedPtr(_) => {
            "a pointer"
        }
        Value::NotAnAddress(_) => "a value in a pointer slot",
    }
}

/// What the runtime does not do yet.
pub mod not_yet {
    /// A payload reaches a guest only inside its file's bytes; GPU access has no capability.
    pub const PAYLOADS: &str =
        "a payload is part of the file's bytes, and no capability grants a texture or a buffer apart from it";

    /// No call builds a tree of the document: it would be proportional to the
    /// file and live in the host's memory at a guest's request. A guest that
    /// wants the whole document reads the file's bytes and parses them itself.
    pub const WHOLE_DOCUMENT: &str = "no host call reads more than one described field, by design";
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `match` arm in `run` missing from `HOST_FUNCTIONS` would be registered
    /// with no capability; a table entry with no arm fails at run time.
    #[test]
    fn every_registered_function_belongs_to_a_capability() {
        let source = include_str!("runtime.rs");
        for host_fn in HOST_FUNCTIONS {
            assert!(
                source.contains(&format!("\"{}\" => {{", host_fn.name)),
                "`{}` is declared in HOST_FUNCTIONS with no match arm in `run`",
                host_fn.name
            );
        }

        // The other direction: a quoted arm that is not in the table.
        for line in source.lines().map(str::trim) {
            let Some(name) = line
                .strip_prefix('"')
                .and_then(|rest| rest.split('"').next())
            else {
                continue;
            };
            if !line.contains("\" => {") {
                continue;
            }
            assert!(
                HOST_FUNCTIONS.iter().any(|f| f.name == name),
                "`{name}` is registered in `run` and absent from HOST_FUNCTIONS, so \
                 it is a host function no capability pays for — which is how a \
                 capability model gets defeated by convenience"
            );
        }
    }

    /// Every closure registered with `func_wrap` ends in `status.code()`, so no
    /// return value carries both an answer and a refusal.
    #[test]
    fn a_host_function_returns_a_status_and_never_an_answer() {
        let source = include_str!("runtime.rs");
        let returns = source
            .lines()
            .filter(|line| line.trim() == "status.code()")
            .count();
        assert_eq!(
            returns,
            HOST_FUNCTIONS.len(),
            "there are {} host functions and {returns} lines returning a status; a \
             function answering with anything else is putting data in the slot a \
             refusal uses",
            HOST_FUNCTIONS.len()
        );
    }

    /// A rebuild moves everything after an allocation that grew, and a guest must
    /// not be able to ask for one, so this crate never names the rebuilding entry
    /// points of `or_io::write`.
    #[test]
    fn the_host_patches_and_never_builds() {
        for file in ["runtime.rs", "host.rs", "lib.rs", "abi.rs", "manifest.rs"] {
            let source = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src")
                    .join(file),
            )
            .expect("every module of this crate");
            // ⚠ Spelled in halves so this test is not itself the match it
            // forbids. A source scan that finds its own needle passes for the
            // wrong reason on the day the code is clean.
            for forbidden in [
                concat!("or_io::write::", "build"),
                concat!("PgRsc", "Builder"),
                concat!("build", "_into"),
            ] {
                assert!(
                    !source.contains(forbidden),
                    "`src/{file}` names `{forbidden}`. A patch moves nothing and a \
                     build moves everything after whatever grew; a plugin surface \
                     that can reach the second is a plugin surface that can \
                     relocate a file's whole heap."
                );
            }
        }
    }

    #[test]
    fn the_default_limits_bound_a_guest() {
        let limits = Limits::default();
        assert!(
            limits.fuel.is_some(),
            "an unbounded guest is a frozen editor, which is the failure WASM does \
             not prevent on its own"
        );
        assert!(limits.memory_pages > 0);
    }

    #[test]
    fn the_functions_a_grant_buys_are_derivable() {
        for writing in ["patch_field", "write_file", "write_files"] {
            assert!(
                !Runtime::functions_for(&Grants::default()).contains(&writing),
                "⚠ the default grant reads the described document only, so `{writing}` \
                 must not be among what it buys"
            );
        }
        assert_eq!(
            Runtime::functions_for(&Grants::default()).len(),
            HOST_FUNCTIONS
                .iter()
                .filter(|f| f.needs == Capability::ReadOpenDocument)
                .count(),
            "and it buys every function that reads the described document, and no other: \
             a file's bytes are asked for"
        );
        assert!(
            Runtime::functions_for(&Grants::none()).is_empty(),
            "no grants, no functions — which is what makes an ungranted capability \
             an unresolved import"
        );
        assert_eq!(
            Runtime::capability_for("patch_field"),
            Some(Capability::EditOpenDocument)
        );
        assert_eq!(Runtime::capability_for("nonsense"), None);
    }
}
