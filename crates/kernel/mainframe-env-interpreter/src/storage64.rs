//! Checked virtual storage for the non-LE AMODE(64) CICS boundary.
//!
//! Addresses in this module are opaque, eight-byte, big-endian virtual identities.
//! They are never native pointers. The high nibble separates them from the
//! interpreter's COBOL pointer encoding, including its eight-byte form.

use std::collections::BTreeMap;

const ADDRESS_PREFIX: u64 = 0xA000_0000_0000_0000;
const ADDRESS_ID_LIMIT: u32 = 0x0fff_ffff;
const MAX_IBM_LENGTH: u32 = 2_146_435_056;
const LOC24_START: u64 = 0x0000_1000;
const LOC24_END: u64 = 0x0100_0000;
const LOC31_START: u64 = 0x0100_0000;
const LOC31_END: u64 = 0x8000_0000;

/// The DSA location selected by GETMAIN64. The virtual address itself does not
/// claim a native below-line, below-bar, or above-bar numerical address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Storage64Location {
    AboveBar,
    Loc24,
    Loc31,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Storage64Key {
    User,
    Cics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Storage64Attributes {
    pub location: Storage64Location,
    pub key: Storage64Key,
    pub shared: bool,
    pub executable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Storage64Limits {
    pub max_allocations: u32,
    pub max_bytes: u64,
}

/// Exact source conditions and an explicit failure for an invalid checkpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Storage64Problem {
    Length,
    NoStorage,
    ExecutableAboveBar,
    InvalidPointer,
    KeyViolation,
    InvalidAbi,
    InvalidSnapshot,
}

impl Storage64Problem {
    #[must_use]
    pub const fn response(self) -> Option<(u16, u16)> {
        match self {
            Self::Length => Some((22, 1)),
            Self::NoStorage => Some((42, 2)),
            Self::ExecutableAboveBar => Some((16, 2)),
            Self::InvalidPointer => Some((16, 1)),
            Self::KeyViolation => Some((16, 2)),
            Self::InvalidAbi | Self::InvalidSnapshot => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Storage64Allocation {
    pub address: u64,
    pub owner: String,
    pub attributes: Storage64Attributes,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Storage64Snapshot {
    pub next_id: u32,
    pub next_loc24: u64,
    pub next_loc31: u64,
    pub allocations: Vec<Storage64Allocation>,
}

/// Monotonic allocation identities make a freed pointer stale for the lifetime
/// of the arena and across checkpoint restoration. A live area's charged size
/// includes 16-byte rounding and the two eight-byte task crumple zones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Storage64Arena {
    limits: Storage64Limits,
    next_id: u32,
    next_loc24: u64,
    next_loc31: u64,
    allocations: BTreeMap<u64, Storage64Allocation>,
}

impl Storage64Arena {
    #[must_use]
    pub fn new(limits: Storage64Limits) -> Self {
        Self {
            limits,
            next_id: 1,
            next_loc24: LOC24_START,
            next_loc31: LOC31_START,
            allocations: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Storage64Snapshot {
        Storage64Snapshot {
            next_id: self.next_id,
            next_loc24: self.next_loc24,
            next_loc31: self.next_loc31,
            allocations: self.allocations.values().cloned().collect(),
        }
    }

    pub fn restore(&mut self, snapshot: Storage64Snapshot) -> Result<(), Storage64Problem> {
        if snapshot.next_id == 0
            || snapshot.next_id > ADDRESS_ID_LIMIT + 1
            || !(LOC24_START..=LOC24_END).contains(&snapshot.next_loc24)
            || !(LOC31_START..=LOC31_END).contains(&snapshot.next_loc31)
            || (snapshot.next_loc24 - LOC24_START) % 16 != 0
            || (snapshot.next_loc31 - LOC31_START) % 16 != 0
        {
            return Err(Storage64Problem::InvalidSnapshot);
        }
        let mut restored = Self::new(self.limits);
        restored.next_id = snapshot.next_id;
        restored.next_loc24 = snapshot.next_loc24;
        restored.next_loc31 = snapshot.next_loc31;
        for allocation in snapshot.allocations {
            let valid_address = match allocation.attributes.location {
                Storage64Location::AboveBar => {
                    address_id(allocation.address).is_some_and(|id| id < restored.next_id)
                }
                Storage64Location::Loc24 => valid_below_bar_address(
                    allocation.address,
                    allocation.bytes.len(),
                    LOC24_START,
                    restored.next_loc24,
                ),
                Storage64Location::Loc31 => valid_below_bar_address(
                    allocation.address,
                    allocation.bytes.len(),
                    LOC31_START,
                    restored.next_loc31,
                ),
            };
            if !valid_address
                || allocation.bytes.is_empty()
                || allocation.bytes.len() > MAX_IBM_LENGTH as usize
                || allocation.owner.is_empty()
                || (allocation.attributes.executable
                    && allocation.attributes.location == Storage64Location::AboveBar)
                || restored
                    .allocations
                    .insert(allocation.address, allocation)
                    .is_some()
            {
                return Err(Storage64Problem::InvalidSnapshot);
            }
        }
        if restored.allocations.len() > self.limits.max_allocations as usize
            || restored
                .charged_bytes()
                .is_none_or(|bytes| bytes > self.limits.max_bytes)
            || live_ranges_overlap(restored.allocations.values())
        {
            return Err(Storage64Problem::InvalidSnapshot);
        }
        *self = restored;
        Ok(())
    }

    #[must_use]
    pub fn available_bytes(&self) -> u64 {
        self.limits
            .max_bytes
            .saturating_sub(self.charged_bytes().unwrap_or(u64::MAX))
    }

    #[must_use]
    pub fn live_allocations(&self) -> usize {
        self.allocations.len()
    }

    #[must_use]
    pub fn location_capacity(&self, location: Storage64Location) -> Option<(u64, u64)> {
        match location {
            Storage64Location::AboveBar => None,
            Storage64Location::Loc24 => {
                Some((LOC24_END - LOC24_START, LOC24_END - self.next_loc24))
            }
            Storage64Location::Loc31 => {
                Some((LOC31_END - LOC31_START, LOC31_END - self.next_loc31))
            }
        }
    }

    pub fn allocate(
        &mut self,
        owner: &str,
        length: i64,
        attributes: Storage64Attributes,
    ) -> Result<u64, Storage64Problem> {
        if length <= 0 || length > i64::from(MAX_IBM_LENGTH) {
            return Err(Storage64Problem::Length);
        }
        if attributes.executable && attributes.location == Storage64Location::AboveBar {
            return Err(Storage64Problem::ExecutableAboveBar);
        }
        let length = u32::try_from(length).map_err(|_| Storage64Problem::Length)?;
        let charge = charged_length(length).ok_or(Storage64Problem::NoStorage)?;
        if self
            .location_capacity(attributes.location)
            .is_some_and(|(limit, _)| charge > limit)
        {
            return Err(Storage64Problem::Length);
        }
        if owner.is_empty()
            || self.allocations.len() >= self.limits.max_allocations as usize
            || charge > self.available_bytes()
            || self.next_id > ADDRESS_ID_LIMIT
        {
            return Err(Storage64Problem::NoStorage);
        }
        let address = match attributes.location {
            Storage64Location::AboveBar => {
                let address = ADDRESS_PREFIX | (u64::from(self.next_id) << 32) | 8;
                self.next_id += 1;
                address
            }
            Storage64Location::Loc24 => {
                if self
                    .next_loc24
                    .checked_add(charge)
                    .is_none_or(|end| end > LOC24_END)
                {
                    return Err(Storage64Problem::NoStorage);
                }
                let address = self.next_loc24 + 8;
                self.next_loc24 += charge;
                address
            }
            Storage64Location::Loc31 => {
                if self
                    .next_loc31
                    .checked_add(charge)
                    .is_none_or(|end| end > LOC31_END)
                {
                    return Err(Storage64Problem::NoStorage);
                }
                let address = self.next_loc31 + 8;
                self.next_loc31 += charge;
                address
            }
        };
        self.allocations.insert(
            address,
            Storage64Allocation {
                address,
                owner: owner.into(),
                attributes,
                bytes: vec![0; length as usize],
            },
        );
        Ok(address)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn get(&self, address: u64) -> Option<&Storage64Allocation> {
        self.allocations.get(&address)
    }

    #[must_use]
    pub(crate) fn contains(&self, address: u64) -> bool {
        self.allocations.contains_key(&address)
    }

    pub(crate) fn can_release(
        &self,
        address: u64,
        task: &str,
        caller_key: Storage64Key,
    ) -> Result<(), Storage64Problem> {
        self.access(address, task, caller_key).map(|_| ())
    }

    pub fn read(
        &self,
        address: u64,
        offset: usize,
        length: usize,
        task: &str,
        caller_key: Storage64Key,
    ) -> Result<Vec<u8>, Storage64Problem> {
        let allocation = self.access(address, task, caller_key)?;
        let end = offset
            .checked_add(length)
            .ok_or(Storage64Problem::InvalidPointer)?;
        allocation
            .bytes
            .get(offset..end)
            .map(<[u8]>::to_vec)
            .ok_or(Storage64Problem::InvalidPointer)
    }

    pub(crate) fn available_length(
        &self,
        address: u64,
        task: &str,
        caller_key: Storage64Key,
    ) -> Result<usize, Storage64Problem> {
        Ok(self.access(address, task, caller_key)?.bytes.len())
    }

    pub fn write(
        &mut self,
        address: u64,
        offset: usize,
        value: &[u8],
        task: &str,
        caller_key: Storage64Key,
    ) -> Result<(), Storage64Problem> {
        self.access(address, task, caller_key)?;
        let end = offset
            .checked_add(value.len())
            .ok_or(Storage64Problem::InvalidPointer)?;
        self.allocations
            .get_mut(&address)
            .and_then(|allocation| allocation.bytes.get_mut(offset..end))
            .ok_or(Storage64Problem::InvalidPointer)?
            .copy_from_slice(value);
        Ok(())
    }

    /// Only the acquiring task can free private storage. Shared storage has
    /// explicit lifetime. CICS-key storage requires a CICS-key caller.
    pub fn release(
        &mut self,
        address: u64,
        task: &str,
        caller_key: Storage64Key,
    ) -> Result<(), Storage64Problem> {
        self.can_release(address, task, caller_key)?;
        self.allocations.remove(&address);
        Ok(())
    }

    fn access(
        &self,
        address: u64,
        task: &str,
        caller_key: Storage64Key,
    ) -> Result<&Storage64Allocation, Storage64Problem> {
        let allocation = self
            .allocations
            .get(&address)
            .ok_or(Storage64Problem::InvalidPointer)?;
        if !allocation.attributes.shared && allocation.owner != task {
            return Err(Storage64Problem::InvalidPointer);
        }
        if allocation.attributes.key == Storage64Key::Cics && caller_key == Storage64Key::User {
            return Err(Storage64Problem::KeyViolation);
        }
        Ok(allocation)
    }

    pub fn end_task(&mut self, task: &str) {
        self.allocations
            .retain(|_, allocation| allocation.attributes.shared || allocation.owner != task);
    }

    #[must_use]
    pub fn charged_bytes(&self) -> Option<u64> {
        self.allocations
            .values()
            .try_fold(0u64, |total, allocation| {
                total.checked_add(charged_length(allocation.bytes.len() as u32)?)
            })
    }
}

fn charged_length(length: u32) -> Option<u64> {
    u64::from(length)
        .checked_add(15)
        .map(|value| value & !15)?
        .checked_add(16)
}

fn address_id(address: u64) -> Option<u32> {
    if address & 0xF000_0000_ffff_ffff != ADDRESS_PREFIX | 8 {
        return None;
    }
    let id = u32::try_from((address >> 32) & 0x0fff_ffff).ok()?;
    (id != 0).then_some(id)
}

fn valid_below_bar_address(address: u64, length: usize, start: u64, next: u64) -> bool {
    let Ok(length) = u32::try_from(length) else {
        return false;
    };
    address.checked_sub(8).is_some_and(|begin| {
        begin >= start
            && (begin - start) % 16 == 0
            && charged_length(length)
                .and_then(|charge| begin.checked_add(charge))
                .is_some_and(|end| end <= next)
    })
}

fn live_ranges_overlap<'a>(allocations: impl Iterator<Item = &'a Storage64Allocation>) -> bool {
    let mut ranges = allocations
        .filter(|allocation| allocation.attributes.location != Storage64Location::AboveBar)
        .filter_map(|allocation| {
            let begin = allocation.address.checked_sub(8)?;
            let end = begin.checked_add(charged_length(allocation.bytes.len() as u32)?)?;
            Some((begin, end))
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    ranges.windows(2).any(|pair| pair[0].1 > pair[1].0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Storage64Limits = Storage64Limits {
        max_allocations: 2,
        max_bytes: 96,
    };
    const PRIVATE: Storage64Attributes = Storage64Attributes {
        location: Storage64Location::AboveBar,
        key: Storage64Key::User,
        shared: false,
        executable: false,
    };

    #[test]
    fn separate_address_space_capacity_and_stale_identity_survive_restore() {
        let mut arena = Storage64Arena::new(LIMITS);
        let first = arena.allocate("task-a", 17, PRIVATE).unwrap();
        assert_eq!(first >> 60, 0xA);
        arena
            .write(first, 2, b"XY", "task-a", Storage64Key::User)
            .unwrap();
        assert_eq!(
            arena
                .read(first, 1, 4, "task-a", Storage64Key::User)
                .unwrap(),
            b"\0XY\0"
        );
        assert_eq!(
            arena.read(first, 16, 2, "task-a", Storage64Key::User),
            Err(Storage64Problem::InvalidPointer)
        );
        assert_eq!(
            arena.write(first, 0, b"X", "task-b", Storage64Key::User),
            Err(Storage64Problem::InvalidPointer)
        );
        assert_eq!(arena.available_bytes(), 48);
        assert_eq!(
            arena.allocate("task-a", 33, PRIVATE),
            Err(Storage64Problem::NoStorage)
        );
        let snapshot = arena.snapshot();
        let mut restored = Storage64Arena::new(LIMITS);
        restored.restore(snapshot).unwrap();
        assert_eq!(restored.get(first).unwrap().bytes.len(), 17);
        assert_eq!(
            restored
                .read(first, 2, 2, "task-a", Storage64Key::User)
                .unwrap(),
            b"XY"
        );
        restored
            .release(first, "task-a", Storage64Key::User)
            .unwrap();
        assert_eq!(
            restored.release(first, "task-a", Storage64Key::User),
            Err(Storage64Problem::InvalidPointer)
        );
        assert_eq!(
            restored.read(first, 0, 1, "task-a", Storage64Key::User),
            Err(Storage64Problem::InvalidPointer)
        );
        let second = restored.allocate("task-a", 17, PRIVATE).unwrap();
        assert_ne!(first, second);
        assert_eq!(restored.available_bytes(), 48);
        let snapshot = restored.snapshot();
        let mut reopened = Storage64Arena::new(LIMITS);
        reopened.restore(snapshot).unwrap();
        assert!(reopened.get(first).is_none());
        assert!(reopened.get(second).is_some());
    }

    #[test]
    fn conditions_ownership_keys_and_task_end() {
        let mut arena = Storage64Arena::new(Storage64Limits {
            max_allocations: 3,
            max_bytes: 128,
        });
        assert_eq!(
            arena.allocate("a", 0, PRIVATE),
            Err(Storage64Problem::Length)
        );
        assert_eq!(
            arena.allocate("a", i64::from(MAX_IBM_LENGTH) + 1, PRIVATE),
            Err(Storage64Problem::Length)
        );
        assert_eq!(
            arena.allocate(
                "a",
                1,
                Storage64Attributes {
                    executable: true,
                    ..PRIVATE
                }
            ),
            Err(Storage64Problem::ExecutableAboveBar)
        );
        let private = arena.allocate("a", 1, PRIVATE).unwrap();
        assert_eq!(
            arena.release(private, "b", Storage64Key::User),
            Err(Storage64Problem::InvalidPointer)
        );
        let shared = arena
            .allocate(
                "a",
                1,
                Storage64Attributes {
                    location: Storage64Location::Loc31,
                    key: Storage64Key::Cics,
                    shared: true,
                    executable: true,
                },
            )
            .unwrap();
        assert!((LOC31_START..LOC31_END).contains(&shared));
        assert_eq!(
            arena.release(shared, "b", Storage64Key::User),
            Err(Storage64Problem::KeyViolation)
        );
        arena.end_task("a");
        assert!(arena.get(private).is_none());
        assert!(arena.get(shared).is_some());
        arena.release(shared, "b", Storage64Key::Cics).unwrap();
    }

    #[test]
    fn corrupt_checkpoint_is_rejected_without_mutating_live_state() {
        let mut arena = Storage64Arena::new(LIMITS);
        let address = arena.allocate("a", 1, PRIVATE).unwrap();
        let mut snapshot = arena.snapshot();
        snapshot.next_id = 1;
        assert_eq!(
            arena.restore(snapshot),
            Err(Storage64Problem::InvalidSnapshot)
        );
        assert!(arena.get(address).is_some());
    }

    #[test]
    fn location_ranges_are_numeric_and_do_not_reuse_freed_addresses() {
        let mut arena = Storage64Arena::new(Storage64Limits {
            max_allocations: 3,
            max_bytes: 128,
        });
        let loc24 = arena
            .allocate(
                "a",
                1,
                Storage64Attributes {
                    location: Storage64Location::Loc24,
                    ..PRIVATE
                },
            )
            .unwrap();
        let loc31 = arena
            .allocate(
                "a",
                1,
                Storage64Attributes {
                    location: Storage64Location::Loc31,
                    ..PRIVATE
                },
            )
            .unwrap();
        let above = arena.allocate("a", 1, PRIVATE).unwrap();
        assert!(loc24 < 0x0100_0000);
        assert!((0x0100_0000..0x8000_0000).contains(&loc31));
        assert!(above > 0xffff_ffff);
        arena.release(loc24, "a", Storage64Key::User).unwrap();
        let snapshot = arena.snapshot();
        let mut restored = Storage64Arena::new(Storage64Limits {
            max_allocations: 3,
            max_bytes: 128,
        });
        restored.restore(snapshot).unwrap();
        let next = restored
            .allocate(
                "a",
                1,
                Storage64Attributes {
                    location: Storage64Location::Loc24,
                    ..PRIVATE
                },
            )
            .unwrap();
        assert_ne!(loc24, next);
        assert_eq!(
            restored.release(loc24, "a", Storage64Key::User),
            Err(Storage64Problem::InvalidPointer)
        );
        assert!(restored.get(loc31).is_some());
    }
}
