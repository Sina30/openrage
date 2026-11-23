use std::collections::HashMap;
use std::path::Path;
use std::sync::{OnceLock, RwLock};

use super::format_handlers::{FormatId, Tier};
use or_io::container::resource::ResourceType;
use or_io::decl::{Gen8, Gen9, Repair};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::gen9::GrcTextureG9;
use or_io::gta5::script::ScrProgram;
use or_io::gta5::File as _;
use or_io::gta5::{
    CNavMesh, CPathRegion, CVehicleRecording, CharacterCloth, CrClipDictionary,
    CrExpressionsDictionary, CrFrameFilterDictionary, FwWaypointRecordingRoute, GtaDrawable,
    GtaFragType, PgDictionary, PhBoundAny, PtxFxList,
};
use or_io::{Asset, Generation};

use super::document::{DocPayload, ScriptDocument};

pub type ParseFn = fn(&Path, &[u8]) -> Result<DocPayload, String>;

/// The bytes, rewriting a count a resource states against its run as the [`Repair`] allows, and each count it rewrote.
pub type SerializeFn = fn(&DocPayload, Repair) -> Result<(Vec<u8>, Vec<String>), String>;

#[derive(Clone, Copy)]
pub struct PayloadHandler {
    pub id: FormatId,
    pub parse: ParseFn,
    pub serialize: SerializeFn,
    /// A blank document of this format, built from a compiled default.
    pub empty: fn() -> DocPayload,
}

impl PayloadHandler {
    /// The bytes, refusing a count a resource states against its run.
    pub fn save(&self, payload: &DocPayload) -> Result<Vec<u8>, String> {
        (self.serialize)(payload, Repair::Refuse).map(|(bytes, _)| bytes)
    }
}

pub(super) static HANDLERS: &[PayloadHandler] = &[
    PayloadHandler {
        id: FormatId::new(ResourceType::Ypt, Generation::Gen8),
        parse: |_path, data| {
            let mut list = PtxFxList::<Gen8>::from_bytes(data).map_err(parse_error)?;
            or_particle::list::canonical(&mut list);
            Ok(DocPayload::Resource(Asset::Particles(list)))
        },
        serialize: |payload, repair| match payload {
            DocPayload::Resource(Asset::Particles(v)) => {
                let mut list = v.clone();
                or_particle::list::canonical(&mut list);
                list.to_bytes_repairing(repair).map_err(save_error)
            }
            other => Err(wrong_payload("Particles", other)),
        },
        empty: || DocPayload::Resource(Asset::Particles(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ypt, Generation::Gen9),
        parse: |_path, data| {
            let mut list = PtxFxList::<Gen9>::from_bytes(data).map_err(parse_error)?;
            or_particle::list::canonical(&mut list);
            Ok(DocPayload::Resource(Asset::ParticlesGen9(list)))
        },
        serialize: |payload, repair| match payload {
            DocPayload::Resource(Asset::ParticlesGen9(v)) => {
                let mut list = v.clone();
                or_particle::list::canonical(&mut list);
                list.to_bytes_repairing(repair).map_err(save_error)
            }
            other => Err(wrong_payload("ParticlesGen9", other)),
        },
        empty: || DocPayload::Resource(Asset::ParticlesGen9(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ytd, Generation::Gen8),
        parse: |_path, data| {
            PgDictionary::<GrcTexture>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::TextureDict(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::TextureDict(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ytd, Generation::Gen9),
        parse: |_path, data| {
            PgDictionary::<GrcTextureG9>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::TextureDictGen9(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::TextureDictGen9(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ydr, Generation::Gen8),
        parse: |_path, data| {
            GtaDrawable::<Gen8>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::Drawable(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::Drawable(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ydr, Generation::Gen9),
        parse: |_path, data| {
            GtaDrawable::<Gen9>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::DrawableGen9(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::DrawableGen9(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ydd, Generation::Gen8),
        parse: |_path, data| {
            PgDictionary::<GtaDrawable<Gen8>>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::DrawableDict(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::DrawableDict(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ydd, Generation::Gen9),
        parse: |_path, data| {
            PgDictionary::<GtaDrawable<Gen9>>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::DrawableDictGen9(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::DrawableDictGen9(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Yft, Generation::Gen8),
        parse: |_path, data| {
            GtaFragType::<Gen8>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::Fragment(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::Fragment(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Yft, Generation::Gen9),
        parse: |_path, data| {
            GtaFragType::<Gen9>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::FragmentGen9(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::FragmentGen9(Default::default())),
    },
    // No Gen9 row for `.ybn`, `.yld`, `.ybd` or `.ynv`: each opens at one
    // layout whatever install ships it, so the Legacy handler is the Enhanced one.
    PayloadHandler {
        id: FormatId::new(ResourceType::Ybn, Generation::Gen8),
        parse: |_path, data| {
            PhBoundAny::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::Bounds(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        // A blank bound is a sphere: its type is zero.
        empty: || {
            DocPayload::Resource(Asset::Bounds(PhBoundAny::PhBoundSphere(Default::default())))
        },
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Yld, Generation::Gen8),
        parse: |_path, data| {
            PgDictionary::<CharacterCloth>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::ClothDict(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::ClothDict(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ycd, Generation::Gen8),
        parse: |_path, data| {
            CrClipDictionary::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::Clips(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::Clips(Default::default())),
    },
    // No Gen9 row for these three: both installs ship `.yed` at version 25,
    // `.yfd` at 4 and `.ynd` at 1, so the Legacy handler is the Enhanced one.
    PayloadHandler {
        id: FormatId::new(ResourceType::Yed, Generation::Gen8),
        parse: |_path, data| {
            CrExpressionsDictionary::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::ExpressionDict(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::ExpressionDict(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Yfd, Generation::Gen8),
        parse: |_path, data| {
            CrFrameFilterDictionary::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::FrameFilterDict(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::FrameFilterDict(Default::default())),
    },
    PayloadHandler {
        // No `DocumentKind`: a `.ybd` opens as `DocumentKind::Registered` and
        // shows its described root grid. A retail install ships four `.ybd`
        // and only one of them is at a version this build reads.
        id: FormatId::new(ResourceType::Ybd, Generation::Gen8),
        parse: |_path, data| {
            PgDictionary::<PhBoundAny>::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::BoundsDict(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::BoundsDict(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ynd, Generation::Gen8),
        parse: |_path, data| {
            CPathRegion::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::PathRegion(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::PathRegion(Default::default())),
    },
    PayloadHandler {
        id: FormatId::new(ResourceType::Ynv, Generation::Gen8),
        parse: |_path, data| {
            CNavMesh::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::NavMesh(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::NavMesh(Default::default())),
    },
    PayloadHandler {
        // No `DocumentKind`: a `.yvr` opens as `Registered` and shows its described root grid.
        id: FormatId::new(ResourceType::Yvr, Generation::Gen8),
        parse: |_path, data| {
            CVehicleRecording::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::VehicleRecording(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::VehicleRecording(Default::default())),
    },
    PayloadHandler {
        // No `DocumentKind`: a `.ywr` opens as `Registered` and shows its described root grid.
        id: FormatId::new(ResourceType::Ywr, Generation::Gen8),
        parse: |_path, data| {
            FwWaypointRecordingRoute::from_bytes(data)
                .map(|v| DocPayload::Resource(Asset::WaypointRecording(v)))
                .map_err(parse_error)
        },
        serialize: save_resource,
        empty: || DocPayload::Resource(Asset::WaypointRecording(Default::default())),
    },
    PayloadHandler {
        // No `DocumentKind`: a `.ysc` opens as `Registered`, and both shipped
        // versions read at one layout, so there is no Gen9 row.
        id: FormatId::new(ResourceType::Ysc, Generation::Gen8),
        parse: |_path, data| {
            let program = ScrProgram::from_bytes(data).map_err(parse_error)?;
            ScriptDocument::of(&program)
                .map(ScriptDocument::into_payload)
                .map_err(parse_error)
        },
        // The create menu asks this too, so it shows `.ysc` greyed with this reason.
        serialize: |_, _| {
            Err("a .ysc holds compiled bytecode; OpenRage reads one and does not write one".into())
        },
        empty: || ScriptDocument::default().into_payload(),
    },
];

#[derive(Default)]
pub struct PayloadRegistry {
    tiers: HashMap<Tier, HashMap<FormatId, PayloadHandler>>,
}

impl PayloadRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtin() -> Self {
        let mut registry = Self::new();
        for handler in HANDLERS {
            registry.register(*handler);
        }
        registry
    }

    pub fn register_at(&mut self, tier: Tier, handler: PayloadHandler) {
        self.tiers
            .entry(tier)
            .or_default()
            .insert(handler.id, handler);
    }

    pub fn register(&mut self, handler: PayloadHandler) {
        self.register_at(Tier::Standard, handler);
    }

    pub fn get(&self, resource: ResourceType, target: Generation) -> Option<PayloadHandler> {
        self.exact(FormatId::new(resource, target))
            .or_else(|| self.exact(FormatId::new(resource, Generation::Gen8)))
    }

    fn exact(&self, id: FormatId) -> Option<PayloadHandler> {
        for tier in [Tier::Preferred, Tier::Standard, Tier::Fallback] {
            if let Some(found) = self.tiers.get(&tier).and_then(|m| m.get(&id)) {
                return Some(*found);
            }
        }
        None
    }
}

fn live() -> &'static RwLock<PayloadRegistry> {
    static LIVE: OnceLock<RwLock<PayloadRegistry>> = OnceLock::new();
    LIVE.get_or_init(|| RwLock::new(PayloadRegistry::builtin()))
}

// A registration seam: called by nothing in-tree by definition, and the
// third of the three registrations a format costs. Retires when a plugin
// host calls it.
#[allow(dead_code)]
pub fn register(handler: PayloadHandler) {
    register_at(Tier::Standard, handler);
}

pub fn register_at(tier: Tier, handler: PayloadHandler) {
    live()
        .write()
        .expect("payload table poisoned")
        .register_at(tier, handler);
}

pub fn handler_for(resource: ResourceType, target: Generation) -> Option<PayloadHandler> {
    live()
        .read()
        .expect("payload table poisoned")
        .get(resource, target)
}

pub(super) fn handler_for_payload(payload: &DocPayload) -> Option<PayloadHandler> {
    let resource = payload.kind().resource_type()?;
    handler_for(resource, payload.generation())
}

fn save_resource(payload: &DocPayload, repair: Repair) -> Result<(Vec<u8>, Vec<String>), String> {
    match payload {
        DocPayload::Resource(asset) => asset.to_bytes_repairing(repair).map_err(save_error),
        other => Err(wrong_payload("Resource", other)),
    }
}

fn parse_error(e: impl std::fmt::Display) -> String {
    format!("Parse error: {e}")
}

fn save_error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn wrong_payload(expected: &str, got: &DocPayload) -> String {
    format!(
        "internal: the {expected} writer was handed a {:?} payload",
        got.kind()
    )
}

/// The corpus files of `extension` at the format's Enhanced RSC7 version: an Enhanced install also ships a `.ytd` at `.ynv`'s.
#[cfg(test)]
pub(crate) fn enhanced_corpus(extension: &str) -> Vec<std::path::PathBuf> {
    use or_io::container::resource::{ResourceEnvelope, Rsc7};
    let Some(format) = or_io::gta5::formats::format(extension) else {
        return Vec::new();
    };
    let enhanced = |bytes: &[u8]| {
        Rsc7::peek(bytes)
            .is_ok_and(|seal| (format.target_of_version(seal.version())).is_ok_and(|t| t.is_gen9()))
    };
    or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), extension)
        .target(or_test_support::TARGET_GEN9)
        .find()
        .into_iter()
        .filter(|path| std::fs::read(path).is_ok_and(|bytes| enhanced(&bytes)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest fixture at a version its format reads: the corpus holds `.ybd` at versions this build refuses.
    fn smallest_readable(extension: &str) -> Option<std::path::PathBuf> {
        use or_io::container::resource::{ResourceEnvelope, Rsc7};
        let format = or_io::gta5::formats::format(extension)?;
        let mut files = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), extension).find();
        files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
        files.into_iter().find(|p| {
            std::fs::read(p).is_ok_and(|bytes| {
                Rsc7::peek(&bytes)
                    .is_ok_and(|seal| format.target_of_version(seal.version()).is_ok())
            })
        })
    }

    /// A sealed `.ysc` whose one function calls `native`, in `scrProgram`'s field order.
    fn synthetic_script(native: u64) -> Vec<u8> {
        use or_io::constants::FIXED_VIRTUAL_BASE;
        use or_io::container::resource::{Compression, ResourceEnvelope, Rsc7, YSC_FILE_VERSION};
        use or_io::gta5::script::opcode::{ENTER, LEAVE, NATIVE};

        let code = [ENTER, 0, 2, 0, 0, NATIVE, 1 << 2, 0, 0, LEAVE, 0, 0];
        let code_at = 0x80;
        let table_at = code_at + code.len();
        let natives_at = table_at + 8;
        let name_at = natives_at + 8;
        let addr = |at: usize| FIXED_VIRTUAL_BASE + at as u64;

        let mut bytes = Vec::new();
        for word in [0, 0, addr(table_at)] {
            bytes.extend(word.to_le_bytes());
        }
        for word in [0u32, code.len() as u32, 0, 0, 0, 1] {
            bytes.extend(word.to_le_bytes());
        }
        for word in [0, 0, addr(natives_at), 0, 0] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend([0; 8]);
        bytes.extend(addr(name_at).to_le_bytes());
        bytes.extend([0; 24]);
        assert_eq!(bytes.len(), code_at);
        bytes.extend(code);
        bytes.extend(addr(code_at).to_le_bytes());
        bytes.extend(native.rotate_right(code.len() as u32).to_le_bytes());
        bytes.extend(b"main\0\0\0\0");

        let seal = Rsc7::invent_single_chunk_seal(YSC_FILE_VERSION, bytes.len()).expect("a seal");
        bytes.resize(seal.total_size() as usize, 0);
        Rsc7::reseal(&bytes, &seal, Compression::fast()).expect("sealed")
    }

    #[test]
    fn a_script_opens_as_its_listing_and_says_why_natives_are_hashes() {
        let native = 0x4EDE_34FB_ADD9_67A6u64;
        let path = std::path::Path::new("main.ysc");
        let kind = crate::state::DocumentKind::from_path(path).expect("a .ysc has a kind");
        let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
            std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
        let payload =
            super::super::document_io::parse_payload(path, kind, &synthetic_script(native), crypto)
                .expect("a .ysc opens");
        assert_eq!(payload.kind(), kind);
        let script = ScriptDocument::in_payload(&payload).expect("a .ysc opens as its listing");
        let listing = script.listing.join("\n");
        assert!(
            listing.contains(ScrProgram::native_names_are_unavailable()),
            "{listing}"
        );
        assert!(listing.starts_with("; main: "), "{listing}");
        assert!(
            listing.contains(&format!("0x{native:016X}  ; unnamed")),
            "{listing}"
        );
        let source = script.source.to_string();
        assert!(source.starts_with("// main: decompiled from "), "{source}");
        assert!(source.contains(&format!("_0x{native:016X}(")), "{source}");
        assert!(
            handler_for(ResourceType::Ysc, Generation::Gen8)
                .expect("a handler")
                .save(&payload)
                .is_err(),
            "nothing writes a .ysc"
        );
    }

    /// ⚠ The sentence names the list and its commit, since the list, not OpenRage, is the source of each name.
    #[test]
    fn a_script_names_its_natives_from_a_downloaded_list_and_says_which() {
        use or_io::gta5::script::natives::{NativeDb, NativeDbSet};
        let native = 0x4EDE_34FB_ADD9_67A6u64;
        let program = ScrProgram::from_bytes(&synthetic_script(native)).expect("reads");
        let json = format!(
            r#"{{"BUILTIN": {{"0x{native:016X}": {{"name": "WAIT",
                "params": [{{"type": "int", "name": "ms"}}], "return_type": "void"}}}}}}"#
        );
        let label = "alloc8or/gta5-nativedb-data/natives.json@424fb51b0890";
        let names = NativeDbSet::new(vec![
            NativeDb::parse(label, json.as_bytes()).expect("parses")
        ]);
        let script = ScriptDocument::of_named(&program, &names).expect("decompiles");
        assert!(script.named);
        assert!(script.sentence.contains(label), "{}", script.sentence);
        let listing = script.listing.join("\n");
        assert!(
            listing.contains(&format!("BUILTIN::WAIT(int ms) -> void  ; 0x{native:016X}")),
            "{listing}"
        );
        assert!(
            script.source.to_string().contains("BUILTIN::WAIT("),
            "{}",
            script.source
        );
    }

    #[test]
    fn every_document_kind_that_is_a_resource_has_a_handler() {
        use crate::state::DocumentKind as K;
        for kind in [
            K::Ypt,
            K::Ytd,
            K::Ydr,
            K::Ydd,
            K::Yft,
            K::Ybn,
            K::Yld,
            K::Ycd,
            K::Yed,
            K::Yfd,
            K::Ynd,
            K::Ynv,
        ] {
            let resource = kind
                .resource_type()
                .unwrap_or_else(|| panic!("{kind:?} should be a paged resource"));
            assert!(
                handler_for(resource, Generation::Gen8).is_some(),
                "{kind:?} is openable but has no payload handler"
            );
        }
    }

    #[test]
    fn every_handler_names_a_resource_the_app_can_open() {
        for handler in HANDLERS {
            assert!(
                ResourceType::ALL.contains(&handler.id.resource),
                "{:?} is not a resource the format registry knows",
                handler.id
            );
        }
    }

    #[test]
    fn a_payload_finds_the_handler_that_made_it() {
        // A mismatch means `serialize` would reject its own output.
        for handler in HANDLERS {
            let payload = (handler.empty)();
            let found = handler_for_payload(&payload)
                .unwrap_or_else(|| panic!("{:?} produced an unroutable payload", handler.id));
            assert_eq!(
                found.id, handler.id,
                "{:?}'s empty payload routes to {:?}",
                handler.id, found.id
            );
        }
    }

    #[test]
    fn an_empty_payload_serializes() {
        // A blank that cannot be written turns "New file" into an error, so
        // every compiled default has to survive its own writer.
        // `.ysc` is read-only, so its blank refuses and the menu greys it.
        for handler in HANDLERS
            .iter()
            .filter(|h| h.id.resource != ResourceType::Ysc)
        {
            let payload = (handler.empty)();
            let bytes = handler
                .save(&payload)
                .unwrap_or_else(|e| panic!("{:?} could not write its own empty: {e}", handler.id));
            assert!(
                !bytes.is_empty(),
                "{:?} wrote an empty file for an empty document",
                handler.id
            );
        }
    }

    /// A save applying OpenRage's fixes still runs a `.ypt`'s canonical step, which keys an event rebound by name to its new emitter rule.
    #[test]
    fn a_repairing_save_keeps_a_particle_events_rebinding() {
        let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
            std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
        let mut files = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ypt").find();
        files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
        let rebound = files.iter().find_map(|path| {
            let data = std::fs::read(path).ok()?;
            let kind = crate::state::DocumentKind::Ypt;
            let mut payload =
                super::super::document_io::parse_payload(path, kind, &data, crypto.clone()).ok()?;
            let DocPayload::Resource(Asset::Particles(list)) = &mut payload else {
                return None;
            };
            let names: Vec<String> = or_particle::list::emitter_rules(list)
                .map(|r| r.name.clone())
                .collect();
            let event = or_particle::list::effect_rules_mut(list)
                .flat_map(or_particle::list::events_mut)
                .find(|e| names.iter().any(|n| *n != e.emitter_rule_name))?;
            event.emitter_rule_name = names.into_iter().find(|n| *n != event.emitter_rule_name)?;
            Some((path.clone(), payload))
        });
        let Some((path, payload)) = rebound else {
            return or_test_support::skip_corpus(
                "a_repairing_save_keeps_a_particle_events_rebinding",
                "no Legacy .ypt with an event and two emitter rules staged",
            );
        };
        let mode = crate::state::prefs::RpfOutputMode::Open;
        let plain = super::super::document_io::serialize_payload(&payload, mode, crypto.clone())
            .expect("the rebound .ypt saves");
        let (repaired, _) = super::super::document_io::serialize_payload_repairing(
            &payload,
            mode,
            crypto,
            Repair::Lossless,
        )
        .expect("the rebound .ypt saves with OpenRage's fixes");
        assert!(
            repaired == plain,
            "{}: a save with OpenRage's fixes skipped the .ypt handler's canonical step",
            path.display()
        );
    }

    /// A material constant edited on each corpus Legacy drawable kind saves with no pack and reopens edited.
    #[test]
    fn an_edited_drawable_material_saves_and_reopens_with_no_pack() {
        use or_io::gta5::gen8::EntryData;
        let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
            std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
        let edited = [0.25, 0.5, 0.75, 1.0];

        let mut checked = Vec::new();
        for (extension, kind) in [
            ("ydr", crate::state::DocumentKind::Ydr),
            ("ydd", crate::state::DocumentKind::Ydd),
            ("yft", crate::state::DocumentKind::Yft),
        ] {
            let Some(path) = smallest_readable(extension) else {
                continue;
            };
            let data = std::fs::read(&path).expect("read a staged file");
            let open = |bytes: &[u8]| {
                super::super::document_io::parse_payload(&path, kind, bytes, crypto.clone())
                    .unwrap_or_else(|e| {
                        panic!("{}: does not open with no pack: {e}", path.display())
                    })
            };
            let mut payload = open(&data);

            // The first owner holding a constant, and where the constant is.
            let found = (0..).map_while(|owner| {
                let drawable = crate::state::drawable_mut(&mut payload, owner)?;
                let group = drawable.shader_group.as_deref();
                Some(group.and_then(|g| {
                    g.shaders.iter().enumerate().find_map(|(s, shader)| {
                        (shader.entries.iter())
                            .find(|e| matches!(e.data, EntryData::Float(_)))
                            .map(|e| (owner, s, e.name_hash.get()))
                    })
                }))
            });
            let Some((owner, shader, hash)) = found.flatten().next() else {
                continue;
            };
            let set = |payload: &mut DocPayload| {
                let group = crate::state::drawable_mut(payload, owner)?
                    .shader_group
                    .as_deref_mut()?;
                let EntryData::Float(rows) = &mut group.shaders[shader].entry_mut(hash)?.data
                else {
                    return None;
                };
                let [x, y, z, w] = edited;
                rows[0] = or_io::decl::Vec4 { x, y, z, w };
                Some(())
            };
            set(&mut payload).expect("the constant is there to edit");

            let saved = super::super::document_io::serialize_payload(
                &payload,
                crate::state::prefs::RpfOutputMode::Open,
                crypto.clone(),
            )
            .unwrap_or_else(|e| {
                panic!(
                    "{}: the edit does not save with no pack: {e}",
                    path.display()
                )
            });
            let mut again = open(&saved);
            let group = crate::state::drawable_mut(&mut again, owner)
                .and_then(|d| d.shader_group.as_deref())
                .expect("the reopened drawable keeps its group");
            let Some(EntryData::Float(rows)) = group.shaders[shader].entry(hash).map(|e| &e.data)
            else {
                panic!("{}: the edited constant is gone", path.display());
            };
            let row = rows[0];
            assert_eq!(
                [row.x, row.y, row.z, row.w],
                edited,
                "{}: the edit did not survive the save",
                path.display()
            );
            checked.push(extension);
        }
        if checked.is_empty() {
            return or_test_support::skip_corpus(
                "an_edited_drawable_material_saves_and_reopens_with_no_pack",
                "no Legacy drawable with a material constant staged",
            );
        }
        eprintln!(
            "edited, saved and reopened with no pack: {}",
            checked.join(", ")
        );
    }

    /// Every corpus Enhanced `.ytd`, `.ydr`, `.ydd` and `.yft` opens, saves and reopens equal with no pack.
    #[test]
    fn every_enhanced_row_opens_and_saves_with_no_pack() {
        use crate::state::DocumentKind as K;
        let crypto: std::sync::Arc<dyn or_io::container::RpfCryptoProvider> =
            std::sync::Arc::new(or_io::container::NoOpCryptoProvider::new());
        let mut opened = Vec::new();
        for (extension, kind) in [
            ("ytd", K::Ytd),
            ("ydr", K::Ydr),
            ("ydd", K::Ydd),
            ("yft", K::Yft),
        ] {
            for path in enhanced_corpus(extension) {
                let data = std::fs::read(&path).expect("read a staged file");
                let open = |bytes: &[u8]| {
                    super::super::document_io::parse_payload(&path, kind, bytes, crypto.clone())
                        .unwrap_or_else(|e| {
                            panic!("{}: does not open with no pack: {e}", path.display())
                        })
                };
                let payload = open(&data);
                let DocPayload::Resource(asset) = &payload else {
                    panic!("{}: did not open as an owned value", path.display());
                };
                assert!(
                    payload.generation().is_gen9() && payload.kind() == kind,
                    "{}: opened as a {:?} {:?}",
                    path.display(),
                    payload.generation(),
                    payload.kind()
                );
                let saved = super::super::document_io::serialize_payload(
                    &payload,
                    crate::state::prefs::RpfOutputMode::Open,
                    crypto.clone(),
                )
                .unwrap_or_else(|e| panic!("{}: does not save with no pack: {e}", path.display()));
                let DocPayload::Resource(again) = open(&saved) else {
                    panic!("{}: reopened as another payload", path.display());
                };
                assert!(
                    again == *asset || again.to_bytes().ok() == asset.to_bytes().ok(),
                    "{}: the saved file reopens different",
                    path.display()
                );
                opened.push(path);
            }
        }
        if opened.is_empty() {
            return or_test_support::skip_corpus(
                "every_enhanced_row_opens_and_saves_with_no_pack",
                "no Enhanced .ytd, .ydr, .ydd or .yft staged",
            );
        }
        eprintln!(
            "{} Enhanced files opened, saved and reopened with no pack",
            opened.len()
        );
    }

    #[test]
    fn a_format_without_an_enhanced_variant_falls_back_to_legacy() {
        let gen9 = handler_for(ResourceType::Ybn, Generation::Gen9).expect("a handler");
        assert_eq!(gen9.id.target, Generation::Gen8);

        let ydr = handler_for(ResourceType::Ydr, Generation::Gen9).expect("a handler");
        assert_eq!(ydr.id.target, Generation::Gen9);
    }
}
