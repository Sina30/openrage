use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, OnceLock};

use super::{Described, OpenEnum};
use crate::desc::{Pack, PackEnum, PackType};

/// The closure of a set of declared roots over the types their fields name.
///
/// There is no registration: a type is described exactly when something
/// reachable names it.
pub struct Reach {
    target: String,
    types: BTreeMap<String, PackType>,
    enums: BTreeMap<String, PackEnum>,
    owned: BTreeMap<String, &'static [(&'static str, bool)]>,
    hashes: BTreeMap<String, &'static [&'static str]>,
    problems: Vec<String>,
}

impl Reach {
    pub fn new(target: &str) -> Self {
        Self {
            target: target.to_string(),
            types: BTreeMap::new(),
            enums: BTreeMap::new(),
            owned: BTreeMap::new(),
            hashes: BTreeMap::new(),
            problems: Vec::new(),
        }
    }

    /// The fields an owned type named `name` shows, each with whether it holds an address.
    pub fn owned_public(&self, name: &str) -> Option<&'static [(&'static str, bool)]> {
        self.owned.get(name).copied()
    }

    /// The fields of the type named `name` that are declared a `Hash32`, whatever they are called.
    pub fn hash_fields(&self, name: &str) -> &'static [&'static str] {
        self.hashes.get(name).copied().unwrap_or(&[])
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn visit<R: Described>(&mut self) {
        let () = R::__LAYOUT;
        let name = R::name();
        if self.types.contains_key(&name) {
            return;
        }
        let ty = R::describe();
        if let Some(only) = R::target() {
            if only != self.target {
                self.problems.push(format!(
                    "`{name}` is declared for `{only}` only, and a `{}` description reaches it",
                    self.target
                ));
            }
        }
        if let Some(public) = R::__OWNED_PUBLIC {
            self.owned.insert(name.clone(), public);
        }
        if !R::__HASH_FIELDS.is_empty() {
            self.hashes.insert(name.clone(), R::__HASH_FIELDS);
        }
        self.types.insert(name, ty);
        R::reach(self);
    }

    pub fn visit_enum<E: OpenEnum>(&mut self) {
        self.enums
            .entry(E::name().to_string())
            .or_insert_with(E::describe);
    }

    pub fn types(&self) -> impl Iterator<Item = &PackType> {
        self.types.values()
    }

    pub fn type_by_name(&self, name: &str) -> Option<&PackType> {
        self.types.get(name)
    }

    /// The pack the reached types make, or every problem found reaching them.
    pub fn into_pack(self, title: &str) -> Result<Pack, Vec<String>> {
        if !self.problems.is_empty() {
            return Err(self.problems);
        }
        let mut pack = Pack::new(title, &self.target);
        pack.built_by = format!("or-io {} built-in", env!("CARGO_PKG_VERSION"));
        pack.types = self.types.into_values().collect();
        pack.enums = self.enums.into_values().collect();
        let problems = crate::desc::export::audit(&pack);
        if problems.is_empty() {
            Ok(pack)
        } else {
            Err(problems)
        }
    }
}

/// A root for [`describe`]: every type `R` reaches.
pub fn root<R: Described>(reach: &mut Reach) {
    reach.visit::<R>()
}

/// The built-in description of one title and target, from its declared roots.
pub fn describe(title: &str, target: &str, roots: &[fn(&mut Reach)]) -> Result<Pack, Vec<String>> {
    let mut reach = Reach::new(target);
    for root in roots {
        root(&mut reach);
    }
    reach.into_pack(title)
}

/// A generic type's name, made `'static` once per spelling for the APIs that key on one.
pub fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = names.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use crate::gta5::formats::format;
    use crate::Generation;

    #[test]
    fn a_field_declared_a_hash32_is_said_to_be_one_whatever_it_is_called() {
        let reach = |ext: &str, target| {
            format(ext)
                .and_then(|f| f.reach(target).ok())
                .expect("a declared format reaches")
        };
        let enhanced = reach("ydr", Generation::Gen9);
        assert_eq!(
            enhanced.hash_fields("GrcInstanceDataG9"),
            ["name", "preset"]
        );
        assert_eq!(enhanced.hash_fields("ShaderParamInfo"), ["name"]);
        let legacy = reach("ydr", Generation::Gen8);
        assert_eq!(
            legacy.hash_fields("GrcInstanceData"),
            ["basis_hash", "material_hash"],
            "`basis_hash_code` is a code, not a hash"
        );
        assert!(legacy.hash_fields("GrcInstanceDataG9").is_empty());
    }
}
