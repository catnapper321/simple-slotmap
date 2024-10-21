use std::marker::PhantomData;

/// A 64-bit value that is unique to a value stored in a Slots data
/// structure. Converts to/from a u64 using `From` trait implementations.
/// The phantom type allows the keys from different Slots instances to be
/// identified for compile time checking.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Key<T> {
    index: u32,
    generation: u32,
    _t: PhantomData<T>
}
impl<T> Key<T> {
    fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _t: PhantomData::default()
        }
    }
    fn index(&self) -> u32 {
        self.index
    }
    fn generation(&self) -> u32 {
        self.generation
    }
}
impl<T> PartialEq for Key<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index() == other.index() && self.generation() == other.generation()
    }
}

impl<T> From<u64> for Key<T> {
    fn from(value: u64) -> Self {
        unsafe { std::mem::transmute(value) }
    }
}
impl<T> From<Key<T>> for u64 {
    fn from(value: Key<T>) -> Self {
        unsafe { std::mem::transmute(value) }
    }
}

/// A key-value data structure that stores values in a Vec for O(1)
/// retrievals. Worst case adds are O(n). Adds permanently transfer
/// ownership into the data store; it is not possible to move values out.
/// Keys are weak and versioned: the value in the referenced slot may be
/// dropped at any time, and subsequent retrievals with the same key will
/// fail. Up to u32::MAX generations are supported; overflowing this will
/// cause a panic. Up to u32::MAX slots are supported.
/// The phantom type specifies the `Key` type that will be used by the
/// Slots instance. This provides compile time checking to help prevent
/// keys from different Slots from being used with the wrong instance.
pub struct Slots<K, V> {
    max_slots: usize,
    generation: u32,
    // u32 is the generation number
    data: Vec<(u32, V)>,
    _t: PhantomData<K>
}
impl<K, V> Slots<K, V> {
    pub fn new(initial_slots: usize, max_slots: usize) -> Self {
        let data = Vec::with_capacity(initial_slots);
        Self {
            max_slots,
            generation: 0,
            data,
            _t: PhantomData::default()
        }
    }
    // returns next generation
    fn increment_generation(&mut self) -> u32 {
        if let Some(gen) = self.generation.checked_add(1) {
            self.generation = gen;
            return gen;
        }
        panic!("Generation overflow");
    }
    /// Drops the value stored in the slot. Returns true if the slot was
    /// occupied. Performance is O(1).
    pub fn remove(&mut self, key: Key<K>) -> bool {
        let index = key.index() as usize;
        if let Some((gen, _)) = self.data.get_mut(index) {
            if *gen != 0 && *gen == key.generation {
                *gen = 0;
                return true;
            }
        }
        false
    }
    /// returns a reference for the value at the given key 
    pub fn get(&self, key: Key<K>) -> Option<&V> {
        let index = key.index() as usize;
        if let Some((gen, value)) = self.data.get(index) {
            if *gen != 0 && *gen == key.generation {
                return Some(value);
            }
        }
        None
    }
    /// returns a mutable reference for the value at the given key 
    pub fn get_mut(&mut self, key: Key<K>) -> Option<&mut V> {
        let index = key.index() as usize;
        if let Some((gen, value)) = self.data.get_mut(index) {
            if *gen != 0 && *gen == key.generation {
                return Some(value);
            }
        }
        None
    }
    /// adds a new value, returns the key. Performance is O(n), worst case.
    pub fn add(&mut self, value: V) -> Key<K> {
        let generation = self.increment_generation();
        // linear search for available slot
        for (index, (old_generation, old_value)) in self.data.iter_mut().enumerate() {
            if *old_generation == 0 {
                *old_value = value;
                *old_generation = generation;
                return Key::new(index as u32, generation);
            }
        }
        // need a new slot... ensure that max_slots is not exceeded
        if self.data.len() >= self.max_slots {
            panic!("max slots exceeded");
        }
        // make the new slot
        let index = self.data.len() as u32;
        self.data.push((generation, value));
        Key::new(index, generation)
    }
    /// returns a key that increments the generation, guaranteeing
    /// uniqueness. The index part of the key is set to zero.
    pub fn get_unique_key(&mut self) -> Key<K> {
        let gen = self.increment_generation();
        Key::new(0, gen)
    }
    /// for troubleshooting purposes only...
    pub fn iter(&self) -> std::slice::Iter<(u32, V)> {
        self.data.iter()
    }
}