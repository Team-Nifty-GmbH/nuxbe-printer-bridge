use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Lock a mutex, recovering from poisoning.
///
/// A panic in one background task must not cascade into panics in every other
/// task that touches the same lock; the guarded data is still consistent for
/// our use cases (plain collections, no multi-step invariants).
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Read-lock an `RwLock`, recovering from poisoning.
pub fn read<T>(rwlock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    rwlock.read().unwrap_or_else(|e| e.into_inner())
}

/// Write-lock an `RwLock`, recovering from poisoning.
pub fn write<T>(rwlock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    rwlock.write().unwrap_or_else(|e| e.into_inner())
}
