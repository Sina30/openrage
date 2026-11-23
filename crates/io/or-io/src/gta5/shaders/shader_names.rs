use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

type NameMap = Arc<HashMap<u32, Arc<str>>>;

fn table() -> &'static RwLock<NameMap> {
    static TABLE: OnceLock<RwLock<NameMap>> = OnceLock::new();
    TABLE.get_or_init(|| RwLock::new(Arc::new(HashMap::new())))
}

pub fn install<I, S>(entries: I)
where
    I: IntoIterator<Item = (u32, S)>,
    S: Into<String>,
{
    let mut map: HashMap<u32, Arc<str>> = HashMap::new();
    for (hash, name) in entries {
        let s: String = name.into();
        map.insert(hash, Arc::from(s.into_boxed_str()));
    }
    let new_table = Arc::new(map);
    if let Ok(mut slot) = table().write() {
        *slot = new_table;
    }
}

pub fn lookup(hash: u32) -> Option<Arc<str>> {
    let snapshot = table().read().ok()?.clone();
    snapshot.get(&hash).cloned()
}

pub fn len() -> usize {
    table().read().map(|t| t.len()).unwrap_or(0)
}

pub fn clear() {
    if let Ok(mut slot) = table().write() {
        *slot = Arc::new(HashMap::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_and_snapshot_semantics() {
        clear();
        assert!(lookup(0xDEAD_BEEF).is_none());
        assert_eq!(len(), 0);

        install([
            (0x2143_CD55_u32, "normal_spec_reflect"),
            (0x1234_u32, "foo"),
        ]);
        assert_eq!(lookup(0x2143_CD55).as_deref(), Some("normal_spec_reflect"));
        assert_eq!(lookup(0x1234).as_deref(), Some("foo"));
        assert!(lookup(0xDEAD_BEEF).is_none());
        assert_eq!(len(), 2);

        let snapshot = lookup(0x2143_CD55).expect("entry just installed");
        install([(0x9999_u32, "bar")]);
        assert_eq!(&*snapshot, "normal_spec_reflect");

        assert!(lookup(0x2143_CD55).is_none());
        assert_eq!(lookup(0x9999).as_deref(), Some("bar"));
        assert_eq!(len(), 1);

        clear();
        assert_eq!(len(), 0);
    }
}
