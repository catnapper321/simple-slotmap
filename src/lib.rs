use std::marker::PhantomData;
pub use error::Error;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct KeyInner {
    index: u32,
    generation: u32,
}
/// A 64-bit value that is unique to a value stored in a Slots data
/// structure. Converts to/from a u64 using `From` trait implementations.
/// The phantom type allows the keys from different Slots instances to be
/// identified for compile time checking (if they store different value types)
///
/// The index u32::MAX is reserved, leaving the maximum possible number of
/// addressable slots equal to u32::MAX - 1.
#[derive(Clone, Copy)]
#[repr(C)]
pub union Key<V> {
    x: u64,
    inner: KeyInner,
    _t: PhantomData<V>,
}
impl<V> Key<V> {
    fn new(index: usize, generation: u32) -> Result<Self, Error> {
        if index as u32 == u32::MAX {
            return Err(Error::IndexOutOfBounds)
        }
        Ok(Self {
            inner: KeyInner {
                index: index as u32,
                generation,
            },
        })
    }
    /// returns new key with index = u32::MAX
    fn new_special(generation: u32) -> Result<Self, Error> {
        Ok(Self {
            inner: KeyInner {
                index: u32::MAX,
                generation,
            },
        })
    }
    fn index(&self) -> usize {
        unsafe { self.inner.index as usize }
    }
    fn generation(&self) -> u32 {
        unsafe { self.inner.generation }
    }
}
impl<V> PartialEq for Key<V> {
    fn eq(&self, other: &Self) -> bool {
        unsafe { self.x == other.x }
    }
}

impl<V> From<u64> for Key<V> {
    fn from(value: u64) -> Self {
        Self { x: value }
    }
}
impl<V> From<Key<V>> for u64 {
    fn from(value: Key<V>) -> Self {
        unsafe { value.x }
    }
}
impl<V> std::fmt::Debug for Key<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key <")?;
        unsafe {
            f.write_fmt(format_args!(
                "gen: {}, index: {}",
                self.inner.generation, self.inner.index
            ))
        }?;
        f.write_str(">")
    }
}
impl<V> std::fmt::Display for Key<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// A key-value data structure that stores values in a Vec for O(1)
/// retrievals and additions. Keys are weak and versioned: the value in the
/// referenced slot may be dropped at any time, and subsequent retrievals
/// with the same key will fail. Up to u32::MAX generations are supported.
/// Up to (u32::MAX - 1) slots are supported. 
///
/// This thing is an essentially an allocator that hands out versioned
/// indexes instead of pointers directly into memory.
pub struct SlotMap<V> {
    max_slots: usize,
    generation: u32, // gen number last used to store a value
    data: Vec<Slot<V>>,
    openlist: Vec<usize>, // list of empty slot indexes
}
impl<V> SlotMap<V> {
    pub fn new(initial_slots: u32, max_slots: u32) -> Result<Self, Error> {
        if initial_slots > max_slots { return Err(Error::InvalidArgument) }
        if max_slots == u32::MAX { return Err(Error::InvalidArgument) }
        let data = Vec::with_capacity(initial_slots as usize);
        let openlist = Vec::with_capacity(initial_slots as usize);
        Ok(Self {
            max_slots: max_slots as usize,
            generation: 0, 
            data,
            openlist,
        })
    }
    // returns next generation
    fn increment_generation(&mut self) -> Result<u32, Error> {
        if let Some(gen) = self.generation.checked_add(1) {
            self.generation = gen;
            Ok(gen)
        } else {
            Err(Error::MaxGenerationReached)
        }
    }
    /// Returns the value stored in the slot, or None if the key is out of
    /// date or the slot is empty.
    pub fn remove(&mut self, key: Key<V>) -> Option<V> {
        let index = key.index();
        self.data.get_mut(index)
            .and_then(|slot| slot.remove(key))
            .map(|v| {
                self.openlist.push(index);
                v
            })
    }
    /// Returns a reference for the value at the given key. This is an O(1)
    /// operation.
    pub fn get(&self, key: Key<V>) -> Option<&V> {
        let index = key.index();
        self.data.get(index).and_then(|slot| slot.get(key))
    }
    /// returns a mutable reference for the value at the given key. This is
    /// an O(1) operation.
    pub fn get_mut(&mut self, key: Key<V>) -> Option<&mut V> {
        let index = key.index();
        self.data.get_mut(index).and_then(|slot| slot.get_mut(key))
    }
    /// adds a new value, returns the key.
    pub fn add(&mut self, value: V) -> Result<Key<V>, Error> {
        let generation = self.increment_generation()?;
        if let Some(index) = self.openlist.pop() {
            // reuse an existing empty slot
            if let Some(slot) = self.data.get_mut(index) {
                if slot.is_empty() {
                    // store the value
                    slot.value = value;
                    slot.generation = generation;
                    return Key::new(index, generation);
                } else {
                    return Err(Error::SlotNotEmpty);
                }
            } else {
                return Err(Error::IndexOutOfBounds);
            }
        } else {
            // expand the data vec
            let index = self.data.len();
            if index >= self.max_slots {
                return Err(Error::NoFreeSlots);
            }
            self.data.push(Slot::new(generation, value));
            Key::new(index, generation)
        }
    }
    /// returns a key that increments the generation, guaranteeing
    /// uniqueness. The index part of the key is set to zero. If used with
    /// .get(), it will always result in a None value returned.
    pub fn get_unique_key(&mut self) -> Result<Key<V>, Error> {
        let gen = self.increment_generation()?;
        Key::new_special(gen)
    }
    /// returns number of occupied slots
    pub fn len(&self) -> usize {
        self.data.len() - self.openlist.len()
    }
}

struct Slot<V> {
    generation: u32, // 0 marks an empty slot
    value: V
}
impl<V> Slot<V> {
    fn new(generation: u32, value: V) -> Self {
        Self { generation, value }
    }
    fn is_empty(&self) -> bool {
        self.generation == 0
    }
    // checks the key generation
    fn remove(&mut self, key: Key<V>) -> Option<V> {
        if self.generation > 0 && key.generation() == self.generation {
            let v = unsafe { self.unchecked_remove() };
            Some(v)
        } else {
            None
        }
    }
    unsafe fn unchecked_remove(&mut self) -> V {
        self.generation = 0;
        let swap_value: V = std::mem::zeroed();
        std::mem::replace(&mut self.value, swap_value)
    }
    // checks the generation against the key generation
    fn get(&self, key: Key<V>) -> Option<&V> {
        if self.generation > 0 && key.generation() == self.generation {
            Some(&self.value)
        } else {
            None
        }
    }
    // checks the generation against the key generation
    fn get_mut(&mut self, key: Key<V>) -> Option<&mut V> {
        if self.generation > 0 && key.generation() == self.generation {
            Some(&mut self.value)
        } else {
            None
        }
    }
}

mod error {
    #[derive(Debug, Clone, Copy)]
    pub enum Error {
        IndexOutOfBounds,
        MaxGenerationReached,
        SlotNotEmpty,
        NoFreeSlots,
        InvalidArgument,
    }
    impl std::error::Error for Error {}
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{self:?}")
        }
    }
}

#[cfg(test)]
mod checks {
    use super::*;
    use super::error::Error;

    #[test]
    fn new_slotmap() {
        let x: Result<SlotMap<i32>, Error> = SlotMap::new(4, 10); 
        assert!(x.is_ok());
        let x: Result<SlotMap<i32>, Error> = SlotMap::new(40, 10); 
        assert!(matches!(x, Err(Error::InvalidArgument)));
        let x: Result<SlotMap<i32>, Error> = SlotMap::new(10, 10); 
        assert!(x.is_ok());
        let x: Result<SlotMap<i32>, Error> = SlotMap::new(10, u32::MAX); 
        assert!(matches!(x, Err(Error::InvalidArgument)));
    }
    #[test]
    fn check_slotmap_increment_gen() {
        let mut x: SlotMap<i32> = SlotMap::new(3, 5).unwrap();
        assert_eq!(x.generation, 0);
        let y = x.increment_generation();
        assert_eq!(x.generation, 1);
        assert!(matches!(y, Ok(1)));
        x.generation = u32::MAX;
        let y = x.increment_generation();
        assert!(matches!(y, Err(Error::MaxGenerationReached)));
    }
    #[test]
    fn check_slotmap_add_expand() {
        let mut x: SlotMap<i32> = SlotMap::new(2, 4).unwrap();
        let key = x.add(3);
        unsafe {
            assert!(matches!(key, Ok(Key { inner: KeyInner { index: 0, generation: 1}})));
        }
        let key = x.add(9);
        unsafe {
            assert!(matches!(key, Ok(Key { inner: KeyInner { index: 1, generation: 2}})));
        }
        let key = x.add(1);
        assert!(matches!(key, Ok(_)));
        let key = x.add(2);
        assert!(matches!(key, Ok(_)));
        let key = x.add(99);
        assert!(matches!(key, Err(Error::NoFreeSlots)));
    }
    #[test]
    fn check_slotmap_add_reuse() {
        let mut x: SlotMap<i32> = SlotMap::new(2, 4).unwrap();
        let key1 = x.add(3).unwrap();
        let key2 = x.add(99).unwrap();
        x.remove(key1);
        let key3 = x.add(7);
        unsafe {
            assert!(matches!(key3, Ok(Key { inner: KeyInner {index: 0, ..}})));
        }
    
    }
    #[test]
    fn check_slotmap_remove() {
        let mut x: SlotMap<i32> = SlotMap::new(2, 4).unwrap();
        let key1 = x.add(3).unwrap();
        let key2 = x.add(99).unwrap();
        let removed_value = x.remove(key1);
        assert_eq!(Some(3), removed_value);
        let removed_value = x.remove(key2);
        assert_eq!(Some(99), removed_value);
    }
    #[test]
    fn check_slotmap_get() {
        let mut x: SlotMap<i32> = SlotMap::new(2, 4).unwrap();
        let key1 = x.add(3).unwrap();
        let key2 = x.add(5).unwrap();
        assert!(matches!(x.get(key1), Some(3)));
        assert!(matches!(x.get(key2), Some(5)));
    }
}
