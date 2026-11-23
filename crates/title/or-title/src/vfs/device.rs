pub trait FileDevice: Send + Sync + std::fmt::Debug {
    fn open(&self, relative_path: &str) -> Option<Vec<u8>>;

    /// What [`open`](Self::open) gives, cut to `limit` bytes; a device that can read less of
    /// the file than that does, but never less than a resource's 16-byte header, and an entry
    /// that is decrypted whole (a `.ysc` in an encrypted archive) comes whole.
    ///
    /// ⚠ A resource's front is the front of its stored body, still deflated, so a file cut
    /// short, which `Rsc7::open` opens to the front of its payload.
    fn open_head(&self, relative_path: &str, limit: usize) -> Option<Vec<u8>> {
        let mut bytes = self.open(relative_path)?;
        bytes.truncate(limit);
        Some(bytes)
    }

    fn exists(&self, relative_path: &str) -> bool {
        self.open(relative_path).is_some()
    }

    fn list(&self, _relative_dir: &str) -> Vec<String> {
        Vec::new()
    }

    fn locate(&self, _relative_path: &str) -> Option<String> {
        None
    }

    /// Every file path this device holds, relative to its own root,
    /// **descending through nested archives**.
    ///
    /// ⚠ `None` means *this device cannot enumerate itself*, not *it holds
    /// nothing*: a device that answered `vec![]` for "I do not know" would
    /// make an incomplete glob indistinguishable from an exhaustive one.
    /// [`Vfs::glob`](super::Vfs::glob) reports the mounts it could not walk
    /// beside the matches it found.
    fn walk(&self) -> Option<Vec<String>> {
        None
    }

    /// Drop whatever this device is holding open, so the next call re-reads.
    ///
    /// Archive handles are cached; a file edited under the VFS is otherwise
    /// served from the handle that was opened before the edit.
    fn invalidate(&self) {}
}
