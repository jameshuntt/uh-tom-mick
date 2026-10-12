//! Explicit, independently usable state primitives. This module intentionally
//! does not replace native `Arc`, `Mutex`, or manually authored atomics.
//! Loom selects an instrumented atomic while preserving the implementation.
#[cfg(feature = "loom")]
use loom::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(feature = "loom"))]
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterError {
    Overflow,
    Underflow,
}
impl std::fmt::Display for CounterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Overflow => f.write_str("atomic counter overflow"),
            Self::Underflow => f.write_str("atomic counter underflow"),
        }
    }
}
impl std::error::Error for CounterError {}

/// Metrics-style counter; its Relaxed ordering does NOT publish other data.
/// Operations are checked, avoiding accidental wraparound at either boundary.
#[derive(Debug)]
pub struct AtomicCounter {
    raw: AtomicU64,
}
impl AtomicCounter {
    pub fn new(initial: u64) -> Self {
        Self {
            raw: AtomicU64::new(initial),
        }
    }
    pub fn load(&self) -> u64 {
        self.raw.load(Ordering::Relaxed)
    }
    /// Returns the NEW value, not fetch_add's previous value.
    pub fn add(&self, amount: u64) -> Result<u64, CounterError> {
        self.raw
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                v.checked_add(amount)
            })
            .map(|old| old + amount)
            .map_err(|_| CounterError::Overflow)
    }
    pub fn increment(&self) -> Result<u64, CounterError> {
        self.add(1)
    }
    /// Returns the NEW value. Checked subtraction does not wrap through zero.
    pub fn subtract(&self, amount: u64) -> Result<u64, CounterError> {
        self.raw
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                v.checked_sub(amount)
            })
            .map(|old| old - amount)
            .map_err(|_| CounterError::Underflow)
    }
    pub fn decrement(&self) -> Result<u64, CounterError> {
        self.subtract(1)
    }
    /// Sets the counter to zero, returning the previous value. Concurrent
    /// increments are ordered before or after this swap, not lost mid-update.
    pub fn reset(&self) -> u64 {
        self.raw.swap(0, Ordering::Relaxed)
    }
}
impl Default for AtomicCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagSetError {
    EmptySchema,
    EmptySelection,
    UnknownBits { bits: u64 },
}
impl std::fmt::Display for FlagSetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySchema => f.write_str("flag schema needs at least one bit"),
            Self::EmptySelection => f.write_str("flag operation needs at least one bit"),
            Self::UnknownBits { bits } => write!(f, "unknown flag bits: {bits:#x}"),
        }
    }
}
impl std::error::Error for FlagSetError {}

/// One atomic word holds every flag; RMW operations never tear the snapshot.
/// `known_mask` is immutable, so invalid raw bit assignments are rejected.
#[derive(Debug)]
pub struct AtomicFlagSet {
    raw: AtomicU64,
    known_mask: u64,
}
impl AtomicFlagSet {
    pub fn new(known_mask: u64, initial: u64) -> Result<Self, FlagSetError> {
        if known_mask == 0 {
            return Err(FlagSetError::EmptySchema);
        }
        if initial & !known_mask != 0 {
            return Err(FlagSetError::UnknownBits {
                bits: initial & !known_mask,
            });
        }
        Ok(Self {
            raw: AtomicU64::new(initial),
            known_mask,
        })
    }
    pub fn known_mask(&self) -> u64 {
        self.known_mask
    }
    pub fn bits(&self) -> u64 {
        self.raw.load(Ordering::Acquire)
    }
    fn validate_selection(&self, mask: u64) -> Result<(), FlagSetError> {
        if mask == 0 {
            return Err(FlagSetError::EmptySelection);
        }
        let invalid = mask & !self.known_mask;
        if invalid != 0 {
            return Err(FlagSetError::UnknownBits { bits: invalid });
        }
        Ok(())
    }
    /// Returns the PREVIOUS word; successful RMW is the linearization point.
    pub fn set_bits(&self, mask: u64) -> Result<u64, FlagSetError> {
        self.validate_selection(mask)?;
        Ok(self.raw.fetch_or(mask, Ordering::AcqRel))
    }
    /// Returns the PREVIOUS word.
    pub fn clear_bits(&self, mask: u64) -> Result<u64, FlagSetError> {
        self.validate_selection(mask)?;
        Ok(self.raw.fetch_and(!mask, Ordering::AcqRel))
    }
    /// Returns the PREVIOUS word.
    pub fn toggle_bits(&self, mask: u64) -> Result<u64, FlagSetError> {
        self.validate_selection(mask)?;
        Ok(self.raw.fetch_xor(mask, Ordering::AcqRel))
    }
    pub fn contains_all(&self, mask: u64) -> Result<bool, FlagSetError> {
        self.validate_selection(mask)?;
        Ok(self.bits() & mask == mask)
    }
    pub fn contains_any(&self, mask: u64) -> Result<bool, FlagSetError> {
        self.validate_selection(mask)?;
        Ok(self.bits() & mask != 0)
    }
    /// Validates BOTH words before attempting an atomic whole-word transition.
    /// The inner Err is the currently observed word if the CAS loses the race.
    pub fn compare_exchange(
        &self,
        expected: u64,
        desired: u64,
    ) -> Result<Result<u64, u64>, FlagSetError> {
        let invalid = (expected | desired) & !self.known_mask;
        if invalid != 0 {
            return Err(FlagSetError::UnknownBits { bits: invalid });
        }
        Ok(self
            .raw
            .compare_exchange(expected, desired, Ordering::AcqRel, Ordering::Acquire))
    }
}
