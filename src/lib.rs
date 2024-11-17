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
impl<T> Key<T>
{
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
/// retrievals. Worst case adds are O(n). Adding a value permanently
/// transfers ownership into the data store; it is not possible to move
/// values out. Keys are weak and versioned: the value in the referenced
/// slot may be dropped at any time, and subsequent retrievals with the
/// same key will fail. Up to u32::MAX generations are supported;
/// overflowing this will cause a panic. Up to u32::MAX slots are
/// supported. The phantom type specifies the `Key` type that will be used
/// by the Slots instance. This provides compile time checking to help
/// prevent keys from different Slots from being used with the wrong
/// instance.
///
/// This thing is an essentially an allocator that hands out versioned
/// indexes instead of pointers directly into memory.
pub struct Slots<K: Clone + Copy, V> {
    max_slots: usize,
    generation: u32,
    // u32 is the generation number
    data: Vec<Slot<V>>,
    _t: PhantomData<K>
}
impl<K: Clone + Copy, V> Slots<K, V> {
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
    /// occupied. This is an O(1) operation.
    pub fn remove(&mut self, key: Key<K>) -> bool {
        let index = key.index() as usize;
        if let Some(Slot::Value(gen, _)) = self.data.get(index) {
            if *gen == key.generation {
                self.data[index] = Slot::Empty;
                return true;
            }
        }
        false
    }
    /// Returns a reference for the value at the given key. This is an O(1)
    /// operation.
    pub fn get(&self, key: Key<K>) -> Option<&V> {
        let index = key.index() as usize;
        if let Some(Slot::Value(gen, value)) = self.data.get(index) {
            if *gen == key.generation {
                return Some(value);
            }
        }
        None
    }
    /// returns a mutable reference for the value at the given key. This is
    /// an O(1) operation.
    pub fn get_mut(&mut self, key: Key<K>) -> Option<&mut V> {
        let index = key.index() as usize;
        if let Some(Slot::Value(gen, value)) = self.data.get_mut(index) {
            if *gen == key.generation {
                return Some(value);
            }
        }
        None
    }
    /// Reserves a slot and returns the key to use with a future
    /// `.with_reservation()` function call. This is an O(n) operation.
    pub fn reserve_slot(&mut self) -> Key<K> {
        let generation = self.increment_generation();
        // linear search for available slot
        for (index, slot) in self.data.iter_mut().enumerate() {
            if matches!(slot, Slot::Empty) {
                *slot = Slot::Reserved(generation);
                return Key::new(index as u32, generation);
            }
        }
        // need a new slot... ensure that max_slots is not exceeded
        if self.data.len() >= self.max_slots {
            panic!("max slots exceeded");
        }
        // make the new slot
        let index = self.data.len();
        self.data.push(Slot::Reserved(generation));
        Key::new(index as u32, generation)
    }
    /// Adds the value returned by the closure to the next available slot.
    ///
    /// The closure takes the key as its sole argument, and returns a Result with
    /// the value to insert into the slot. Note that you will need to provide the error
    /// type returned by the closure, for example via turbofish notation:
    /// ```
    /// let new_key = slots.add_with::<MyErrorType>(|key| {
    ///     let thing = Something.new()?;
    ///     Ok(thing)
    /// })?;
    /// ```
    pub fn add_with<E: std::error::Error>(&mut self, f: impl FnOnce(Key<K>) -> Result<V, E>) -> Result<Key<K>, E> {
        let key = self.reserve_slot();
        f(key).map(|v| { self.with_reservation(key, v); key })
    }
    /// adds a new value, returns the key. Performance is O(n), worst case.
    pub fn add(&mut self, value: V) -> Key<K> {
        let key = self.reserve_slot();
        self.data[key.index as usize] = Slot::Value(key.generation, value);
        key
    }
    /// Assigns a value to a reserved slot. This is an O(1) operation.
    pub fn with_reservation(&mut self, key: Key<K>, value: V) {
        if let Some(Slot::Reserved(res_gen)) = self.data.get(key.index as usize) {
            if *res_gen == key.generation {
                self.data[key.index as usize] = Slot::Value(*res_gen, value);
            }
        }
    }
    /// returns a key that increments the generation, guaranteeing
    /// uniqueness. The index part of the key is set to zero.
    pub fn get_unique_key(&mut self) -> Key<K> {
        let gen = self.increment_generation();
        Key::new(0, gen)
    }
    // /// for troubleshooting purposes only...
    // pub fn iter(&self) -> std::slice::Iter<(u32, V)> {
    //     self.data.iter()
    // }
}

enum Slot<V> {
    Empty,
    Reserved(u32),
    Value(u32, V),
}
