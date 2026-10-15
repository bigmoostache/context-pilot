//! `cp-base` foundation contracts at the public-API boundary.
//!
//! * `Safe` casts clamp to the nearer bound (regression: signed positive
//!   overflow into an unsigned target used to collapse to `0`);
//! * the `State` extension map routes first inserts by the ambient init
//!   scope, keeps a type in exactly one scope map, and the forced setters move it;
//! * `YamlSync` merges additively, resolves same-key conflicts by
//!   `last_edited_ms`, and restores a corrupted shared file from its backup.

// The integration target links every `cp-base` dependency but uses only a few;
// Cargo's `as _` form acknowledges the rest for `unused-crate-dependencies`.
use cp_env as _;
use cp_macros as _;
use cp_mod_utilities as _;
use cp_render as _;
use crossterm as _;
use log as _;
use ratatui as _;
use serde_json as _;
use serde_yaml as _;
use unicode_width as _;

// Tests live in a `#[cfg(test)]` module for clippy::tests_outside_test_module
// (same convention as cp-wire/tests/protocol.rs).
#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use cp_base::cast::Safe as _;
    use cp_base::config::yaml_sync::{SyncEntry, YamlSync};
    use cp_base::state::runtime::State;
    use serde::{Deserialize, Serialize};

    // -----------------------------------------------------------------------
    // cast
    // -----------------------------------------------------------------------

    #[test]
    fn signed_positive_overflow_clamps_to_unsigned_max() {
        assert_eq!(300i32.to_u8(), u8::MAX);
        assert_eq!(70_000i32.to_u16(), u16::MAX);
        assert_eq!(i64::MAX.to_u32(), u32::MAX);
        assert_eq!(i64::MAX.to_u8(), u8::MAX);
        assert_eq!(isize::MAX.to_u16(), u16::MAX);
    }

    #[test]
    fn signed_negative_clamps_to_zero() {
        assert_eq!((-1i32).to_u8(), 0);
        assert_eq!(i64::MIN.to_u64(), 0);
        assert_eq!((-5isize).to_usize(), 0);
        assert_eq!(i64::MIN.to_i32(), i32::MIN);
        assert_eq!(i64::MAX.to_i32(), i32::MAX);
    }

    #[test]
    fn unsigned_and_float_clamp() {
        assert_eq!(u64::MAX.to_u8(), u8::MAX);
        assert_eq!(u64::MAX.to_i64(), i64::MAX);
        assert_eq!(1e12f64.to_u32(), u32::MAX);
        assert_eq!((-3.5f64).to_usize(), 0);
        assert_eq!(f64::NAN.to_u8(), 0);
        assert_eq!(42i32.to_u8(), 42);
    }

    // -----------------------------------------------------------------------
    // extension map
    // -----------------------------------------------------------------------

    #[derive(Debug, PartialEq, Eq)]
    struct Shared(u32);

    #[derive(Debug, PartialEq, Eq)]
    struct PerAgent(u32);

    #[test]
    fn ext_first_insert_routes_by_init_scope() {
        let mut state = State::default();
        state.set_init_scope(Some(true));
        state.set_ext(Shared(1));
        state.set_init_scope(None);
        state.set_ext(PerAgent(2));

        let agent_map = &state.agent_store.current().agent_module_data;
        assert!(state.shared_module_data.contains_key(&TypeId::of::<Shared>()));
        assert!(!agent_map.contains_key(&TypeId::of::<Shared>()));
        assert!(agent_map.contains_key(&TypeId::of::<PerAgent>()));
        assert_eq!(state.get_ext::<Shared>(), Some(&Shared(1)));
        assert_eq!(state.get_ext::<PerAgent>(), Some(&PerAgent(2)));
    }

    #[test]
    fn ext_update_stays_in_owning_map() {
        let mut state = State::default();
        state.set_init_scope(Some(true));
        state.set_ext(Shared(1));
        // Scope flips, but an existing type must not migrate or duplicate.
        state.set_init_scope(Some(false));
        state.set_ext(Shared(7));
        state.ext_mut::<Shared>().0 += 1;

        assert_eq!(state.shared_module_data.len(), 1);
        assert!(state.agent_store.current().agent_module_data.is_empty());
        assert_eq!(state.ext::<Shared>(), &Shared(8));
    }

    #[test]
    fn forced_setters_move_between_maps() {
        let mut state = State::default();
        state.set_ext_global(PerAgent(1));
        state.set_ext_agent(PerAgent(2));
        assert!(state.shared_module_data.is_empty());
        assert_eq!(state.get_ext::<PerAgent>(), Some(&PerAgent(2)));

        state.set_ext_global(PerAgent(3));
        assert!(state.agent_store.current().agent_module_data.is_empty());
        assert_eq!(state.get_ext::<PerAgent>(), Some(&PerAgent(3)));
        assert_eq!(state.get_ext::<Shared>(), None);
    }

    // -----------------------------------------------------------------------
    // yaml_sync
    // -----------------------------------------------------------------------

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Note {
        text: String,
        #[serde(default)]
        ts: u64,
    }

    impl SyncEntry for Note {
        fn last_edited_ms(&self) -> u64 {
            self.ts
        }
        fn set_last_edited_ms(&mut self, ms: u64) {
            self.ts = ms;
        }
    }

    fn note(text: &str, ts: u64) -> Note {
        Note { text: text.to_owned(), ts }
    }

    /// Fresh scratch dir + a `YamlSync` whose shared and backup files live in it
    /// (an absolute backup name replaces the `.context-pilot` prefix on join).
    fn scratch(name: &str) -> (PathBuf, YamlSync) {
        let dir = std::env::temp_dir().join(format!("cp-base-yaml-{name}-{}", std::process::id()));
        let _clean = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        let shared = dir.join("shared.yaml");
        let backup = dir.join("backup.yaml");
        let sync = YamlSync::new(&shared.to_string_lossy(), &backup.to_string_lossy());
        (dir, sync)
    }

    #[test]
    fn sync_merges_additively_and_latest_wins() {
        let (dir, sync) = scratch("merge");
        let mut disk = BTreeMap::new();
        let _a = disk.insert("a".to_owned(), note("disk-a", 10));
        let _b = disk.insert("b".to_owned(), note("disk-b-new", 50));
        let _c = disk.insert("c".to_owned(), note("disk-c-old", 5));
        sync.migrate(&disk);

        let mut memory = BTreeMap::new();
        let _b2 = memory.insert("b".to_owned(), note("mem-b-old", 20));
        let _c2 = memory.insert("c".to_owned(), note("mem-c-new", 90));
        let _d = memory.insert("d".to_owned(), note("mem-d", 1));

        assert!(sync.sync(&mut memory));
        assert_eq!(memory.get("a"), Some(&note("disk-a", 10)));
        assert_eq!(memory.get("b"), Some(&note("disk-b-new", 50)));
        assert_eq!(memory.get("c"), Some(&note("mem-c-new", 90)));
        assert_eq!(memory.get("d"), Some(&note("mem-d", 1)));

        assert_eq!(sync.load::<Note>(), memory);
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn corrupted_shared_file_restored_from_backup() {
        let (dir, sync) = scratch("restore");
        let mut entry = note("kept", 0);
        sync.upsert("k", &mut entry);
        assert!(entry.ts > 0);

        fs::write(dir.join("shared.yaml"), "{{ not: valid: yaml").expect("corrupt shared");
        let restored = sync.load::<Note>();
        assert_eq!(restored.get("k").map(|n| n.text.as_str()), Some("kept"));
        let healed = fs::read_to_string(dir.join("shared.yaml")).expect("read healed");
        assert!(healed.contains("kept"));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn remove_where_reports_count() {
        let (dir, sync) = scratch("remove");
        sync.upsert_many(vec![("x".to_owned(), note("1", 0)), ("y".to_owned(), note("2", 0))]);
        assert_eq!(sync.remove_where::<Note, _>(|k, _| k == "x"), 1);
        assert_eq!(sync.remove_where::<Note, _>(|k, _| k == "x"), 0);
        assert_eq!(sync.load::<Note>().len(), 1);
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
