//! Module extension-data accessors for [`State`] (extracted from `runtime/mod.rs`
//! for the 500-line cap).
//!
//! Module-owned state is stored in two scope maps on `State`:
//! `shared_module_data` (fleet-wide, one instance) and `thread_module_data`
//! (per-thread, the executing thread's). A given `TypeId` lives in
//! exactly one map, so the `get_ext` family searches both; `set_ext` updates
//! whichever already holds the type and routes first-inserts by the ambient
//! `init_is_global` scope set by the boot/init loops.

use std::any::TypeId;

use super::State;

#[expect(
    clippy::multiple_inherent_impl,
    reason = "State methods split across runtime/ submodules to respect the 500-line file cap"
)]
impl State {
    /// Get a reference to module-owned state by type.
    ///
    /// Searches both scope maps (a `TypeId` lives in exactly one), so callers
    /// need not know whether the type is fleet-shared or per-thread.
    #[must_use]
    pub fn get_ext<T>(&self) -> Option<&T>
    where
        T: 'static + Send + Sync,
    {
        let id = TypeId::of::<T>();
        let boxed =
            self.thread_store.current().thread_module_data.get(&id).or_else(|| self.shared_module_data.get(&id))?;
        boxed.downcast_ref()
    }

    /// Get a mutable reference to module-owned state by type.
    ///
    /// Searches both scope maps (a `TypeId` lives in exactly one).
    pub fn get_ext_mut<T>(&mut self) -> Option<&mut T>
    where
        T: 'static + Send + Sync,
    {
        let id = TypeId::of::<T>();
        if self.thread_store.current_mut().thread_module_data.contains_key(&id) {
            let boxed = self.thread_store.current_mut().thread_module_data.get_mut(&id)?;
            boxed.downcast_mut()
        } else {
            let boxed = self.shared_module_data.get_mut(&id)?;
            boxed.downcast_mut()
        }
    }

    /// Get module state by type, panicking if not initialized.
    ///
    /// Prefer this over `get_ext().expect()` — the panic lives in
    /// [`invariant_panic`](crate::config::invariant_panic) once,
    /// so callers don't need `expect(clippy::expect_used)`.
    ///
    /// # Panics
    ///
    /// Panics if module state `T` was never registered via [`set_ext`](Self::set_ext).
    #[must_use]
    pub fn ext<T>(&self) -> &T
    where
        T: 'static + Send + Sync,
    {
        self.get_ext::<T>().unwrap_or_else(|| {
            crate::config::invariant_panic("module state not initialized \u{2014} was init_state() called?")
        })
    }

    /// Get mutable module state by type, panicking if not initialized.
    ///
    /// # Panics
    ///
    /// Panics if module state `T` was never registered via [`set_ext`](Self::set_ext).
    pub fn ext_mut<T>(&mut self) -> &mut T
    where
        T: 'static + Send + Sync,
    {
        self.get_ext_mut::<T>().unwrap_or_else(|| {
            crate::config::invariant_panic("module state not initialized \u{2014} was init_state() called?")
        })
    }

    /// Set module-owned state by type. Replaces any existing value of this type.
    ///
    /// If the type is already registered, it is updated in whichever scope map
    /// holds it. For a first insert, the target map is chosen by the ambient
    /// [`init_is_global`](Self::init_is_global) scope (`Some(true)` → shared,
    /// otherwise per-thread) — set by the boot/init loops.
    pub fn set_ext<T>(&mut self, val: T)
    where
        T: 'static + Send + Sync,
    {
        let id = TypeId::of::<T>();
        if self.shared_module_data.contains_key(&id) {
            drop(self.shared_module_data.insert(id, Box::new(val)));
        } else if self.thread_store.current_mut().thread_module_data.contains_key(&id)
            || self.init_is_global != Some(true)
        {
            drop(self.thread_store.current_mut().thread_module_data.insert(id, Box::new(val)));
        } else {
            drop(self.shared_module_data.insert(id, Box::new(val)));
        }
    }

    /// Insert fleet-shared module state, overriding scope routing. Use when a
    /// value must live in [`shared_module_data`](Self::shared_module_data)
    /// regardless of the ambient init scope.
    pub fn set_ext_global<T>(&mut self, val: T)
    where
        T: 'static + Send + Sync,
    {
        let id = TypeId::of::<T>();
        let _thread = self.thread_store.current_mut().thread_module_data.remove(&id);
        drop(self.shared_module_data.insert(id, Box::new(val)));
    }

    /// Insert per-thread module state, overriding scope routing. Use when a
    /// value must live in [`thread_module_data`](Self::thread_module_data)
    /// (the executing thread's) regardless of ambient init scope.
    pub fn set_ext_thread<T>(&mut self, val: T)
    where
        T: 'static + Send + Sync,
    {
        let id = TypeId::of::<T>();
        let _shared = self.shared_module_data.remove(&id);
        drop(self.thread_store.current_mut().thread_module_data.insert(id, Box::new(val)));
    }

    /// Set the ambient scope used to route the next first-insert via
    /// [`set_ext`](Self::set_ext). Called by the boot/init loops around
    /// `Module::init_state` / `load_module_data` (global) and
    /// `load_worker_data` (always per-thread). `None` clears it.
    pub const fn set_init_scope(&mut self, is_global: Option<bool>) {
        self.init_is_global = is_global;
    }
}
