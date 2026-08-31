use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
};

use crate::{bitvec::BitVec, ecs::private::EntityTrait};

mod private {
    use std::{hash::Hash, marker::PhantomData};

    pub trait EntityTrait:
        Clone + Copy + PartialEq + Eq + PartialOrd + Ord + Hash + Unpin + Send + Sync + Sized
    {
        fn id(&self) -> u32;

        fn from_id(id: usize) -> Self;
    }

    impl<M> EntityTrait for super::Entity<M> {
        fn id(&self) -> u32 {
            self.0
        }

        fn from_id(id: usize) -> Self {
            Self(id as u32, PhantomData)
        }
    }
}

pub const INITIAL_ENTITY_CAPACITY: usize = 2500;
pub const ENTITY_CAPACITY_GROWTH_FACTOR: f32 = 1.5;

#[derive(Debug)]
#[repr(transparent)]
pub struct Entity<M>(u32, PhantomData<M>);
impl<M> Clone for Entity<M> {
    fn clone(&self) -> Self {
        Self(self.0, self.1)
    }
}
impl<M> Copy for Entity<M> {}
impl<M> PartialEq for Entity<M> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<M> Eq for Entity<M> {}
impl<M> PartialOrd for Entity<M> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<M> Ord for Entity<M> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}
impl<M> Hash for Entity<M> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl<M> Unpin for Entity<M> {}
unsafe impl<M> Send for Entity<M> {}
unsafe impl<M> Sync for Entity<M> {}

impl<M> Entity<M> {
    pub fn id(&self) -> u32 {
        self.0
    }

    pub fn from_id(id: usize) -> Self {
        Self(id as u32, PhantomData)
    }
}

/// efficiently maintains a set of entities
pub struct EntityManager<E: EntityTrait> {
    /// the list of available entity ids
    free_entity_queue: VecDeque<E>,

    /// the usable entity ids, including free ones
    entities: BitVec,
}

impl<E: EntityTrait> Default for EntityManager<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: EntityTrait> EntityManager<E> {
    pub fn new() -> Self {
        let mut free_entity_queue = VecDeque::with_capacity(INITIAL_ENTITY_CAPACITY);
        for i in 0..INITIAL_ENTITY_CAPACITY {
            free_entity_queue.push_back(E::from_id(i as usize));
        }

        Self {
            entities: BitVec::with_size(INITIAL_ENTITY_CAPACITY),
            free_entity_queue,
        }
    }

    /// gets a new nameless entity, O(1)
    pub fn get_new(&mut self) -> E {
        if self.free_entity_queue.is_empty() {
            let new_count = (self.entities.len() as f32 * ENTITY_CAPACITY_GROWTH_FACTOR)
                .min(usize::MAX as f32) as usize;
            for i in self.entities.len()..new_count {
                self.free_entity_queue.push_back(E::from_id(i as usize));
            }
        }

        self.free_entity_queue.pop_front().unwrap()
    }

    /// checks if an entity is used, O(1)
    /// if the entity came from another entity manager, this function might panic
    pub fn is_used(&self, entity: E) -> bool {
        self.entities.test(entity.id() as usize).unwrap()
    }

    /// returns an entity to the set of unused entities
    /// returns true on success (fails if entity is not used)
    pub fn mark_unused(&mut self, entity: E) -> bool {
        if !self.is_used(entity) {
            return false;
        }
        self.entities.clear(entity.id() as usize);
        self.free_entity_queue.push_back(entity);
        true
    }

    /// gets the number of entities in use, O(1)
    pub fn len(&self) -> usize {
        self.entities.len() - self.free_entity_queue.len()
    }
}

/// efficiently maintains a set of entities potentially with associated names
/// entities can be accessed by their names
pub struct NamedEntityManager<E: EntityTrait> {
    /// the list of available entity ids
    free_entity_queue: VecDeque<E>,

    /// the map from entity to name
    entity_names: DenseComponentManager<E, String>,

    /// the map from name to entity
    name_to_entity_map: HashMap<String, E>,
}

impl<E: EntityTrait> Default for NamedEntityManager<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: EntityTrait> NamedEntityManager<E> {
    /// creates a new entity manager
    pub fn new() -> Self {
        let mut free_entity_queue = VecDeque::with_capacity(INITIAL_ENTITY_CAPACITY);
        for i in 0..INITIAL_ENTITY_CAPACITY {
            free_entity_queue.push_back(E::from_id(i as usize));
        }

        Self {
            free_entity_queue,
            entity_names: DenseComponentManager::new(),
            name_to_entity_map: HashMap::new(),
        }
    }

    /// gets a new nameless entity, O(1)
    pub fn get_new(&mut self) -> E {
        if self.free_entity_queue.is_empty() {
            let new_count = (self.entity_names.len() as f32 * ENTITY_CAPACITY_GROWTH_FACTOR)
                .min(usize::MAX as f32) as usize;
            for i in self.entity_names.len() as usize..new_count {
                self.free_entity_queue.push_back(E::from_id(i));
            }
        }

        self.free_entity_queue.pop_front().unwrap()
    }

    /// creates an entity, assigning it a name, returns None if the name is in use
    /// empty string names are not allowed (returns None)
    pub fn get_new_named(&mut self, name: String) -> Option<E> {
        if name.is_empty() || self.name_to_entity_map.contains_key(&name) {
            return None;
        }

        let entity = self.get_new();
        self.name_to_entity_map.insert(name.clone(), entity);
        self.entity_names.insert(entity, name);

        Some(entity)
    }

    /// gets the entity with the given name, if it exists, average O(1)
    pub fn get_by_name(&self, name: &String) -> Option<E> {
        self.name_to_entity_map.get(name).copied()
    }

    /// gets the name of the given entity, if it exists
    pub fn get_name(&self, entity: E) -> Option<String> {
        self.entity_names.get(entity).cloned()
    }

    /// checks if an entity is used, O(1)
    /// if the entity came from another entity manager, this function might panic
    pub fn is_used(&self, entity: E) -> bool {
        self.entity_names.contains(entity)
    }

    /// checks if a name is used, average O(1)
    pub fn is_name_used(&self, name: &String) -> bool {
        self.name_to_entity_map.contains_key(name)
    }

    /// returns an entity to the set of unused entities
    /// returns true on success (fails if entity is not used)
    pub fn mark_unused(&mut self, entity: E) -> bool {
        if let Some(name) = self.entity_names.remove(entity) {
            self.name_to_entity_map.remove(&name);
            self.free_entity_queue.push_back(entity);
            true
        } else {
            false
        }
    }

    /// gets the number of entities in use, O(1)
    pub fn len(&self) -> usize {
        self.entity_names.len()
    }
}

/// manages a list of contained entities
pub struct DenseSet<E: EntityTrait> {
    set: BitVec,
    marker: PhantomData<E>,
}

impl<E: EntityTrait> Default for DenseSet<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: EntityTrait> DenseSet<E> {
    /// creates a new dense storage
    pub fn new() -> Self {
        Self {
            set: BitVec::with_size(INITIAL_ENTITY_CAPACITY),
            marker: PhantomData,
        }
    }

    /// checks if an entity is in the set
    pub fn contains(&self, entity: E) -> bool {
        if let Some(b) = self.set.test(entity.id() as usize) {
            b
        } else {
            false
        }
    }

    /// adds an entity to the set
    pub fn insert(&mut self, entity: E) {
        // ensure the inner set is the right size
        self.set.expand(entity.id() as usize);

        // add to the set
        self.set.set(entity.id() as usize);
    }

    /// removes an entity from the set
    pub fn remove(&mut self, entity: E) {
        self.set.clear(entity.id() as usize)
    }
}

/// manages a single component for the board's ecs using a dense vector
pub struct DenseComponentManager<E: EntityTrait, T> {
    /// a map entity id -> index in entities and data
    /// uses usize::MAX as a sentenial value for an uninitialized entity
    sparse: Vec<usize>,

    /// a map index -> entity
    entities: Vec<E>,

    /// a map index -> data
    data: Vec<T>,
}

impl<E: EntityTrait, T> Default for DenseComponentManager<E, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: EntityTrait, T> DenseComponentManager<E, T> {
    /// creates a new dense storage
    pub fn new() -> Self {
        Self {
            sparse: vec![usize::MAX; INITIAL_ENTITY_CAPACITY],
            entities: Vec::with_capacity(INITIAL_ENTITY_CAPACITY),
            data: Vec::with_capacity(INITIAL_ENTITY_CAPACITY),
        }
    }

    /// returns the number of entities managed
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// inserts a component into this manager
    pub fn insert(&mut self, entity: E, component: T) {
        // expand sparse if id is too large
        if entity.id() >= self.sparse.len() as u32 {
            self.sparse.resize(entity.id() as usize + 1, usize::MAX);
        }

        // If the entity already has this component, overwrite it
        let idx = self.sparse[entity.id() as usize];
        if idx != usize::MAX {
            self.data[idx] = component;
            return;
        }

        let dense_idx = self.data.len();
        self.entities.push(entity);
        self.data.push(component);
        self.sparse[entity.id() as usize] = dense_idx;
    }

    /// removes an entity from this manager, returning the component that used to be stored there
    pub fn remove(&mut self, entity: E) -> Option<T> {
        // Check if the entity actually has the component
        let remove_idx = *self.sparse.get(entity.id() as usize)?;
        if remove_idx == usize::MAX {
            return None;
        }

        let last_dense_idx = self.data.len() - 1;

        // Swap the target element with the last element in dense arrays
        self.entities.swap(remove_idx, last_dense_idx);
        self.data.swap(remove_idx, last_dense_idx);

        // Update sparse map for the entity that was moved into the new empty slot
        if remove_idx != last_dense_idx {
            let moved_entity = self.entities[remove_idx];
            self.sparse[moved_entity.id() as usize] = remove_idx;
        }

        // Pop the removed element from the end of the vectors
        self.sparse[entity.id() as usize] = usize::MAX;
        self.entities.pop();
        self.data.pop()
    }

    /// checks if the manager contains the given entity
    pub fn contains(&self, entity: E) -> bool {
        if let Some(idx) = self.sparse.get(entity.id() as usize) {
            *idx != usize::MAX
        } else {
            false
        }
    }

    /// queries the manager for an entity
    pub fn get(&self, entity: E) -> Option<&T> {
        let idx = *self.sparse.get(entity.id() as usize)?;
        if idx == usize::MAX {
            return None;
        }

        Some(&self.data[idx])
    }

    /// queries the manager for an entity, mutably
    pub fn get_mut(&mut self, entity: E) -> Option<&T> {
        let idx = *self.sparse.get(entity.id() as usize)?;
        if idx == usize::MAX {
            return None;
        }

        Some(&mut self.data[idx])
    }

    /// returns an iterator over component references
    pub fn components(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    /// returns an iterator over mutable component references
    pub fn components_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.data.iter_mut()
    }

    /// returns an iterator over entities
    pub fn entities(&self) -> impl Iterator<Item = &E> {
        self.entities.iter()
    }

    /// returns an iterator over entities and their associated components
    pub fn pairs(&self) -> impl Iterator<Item = (&E, &T)> {
        self.entities.iter().zip(self.data.iter())
    }

    /// returns an iterator over entities and their associated components mutably
    pub fn pairs_mut(&mut self) -> impl Iterator<Item = (&E, &mut T)> {
        self.entities.iter().zip(self.data.iter_mut())
    }
}

/// efficiently maintains a set of entities potentially with associated names
/// entities can be accessed by their names
pub struct ResourceManager<E: EntityTrait, T> {
    /// the list of available entity ids
    free_entity_queue: VecDeque<E>,

    /// the map from name to entity
    name_to_entity_map: HashMap<String, E>,

    /// the map from entity to index
    sparse: Vec<usize>,

    /// the map from entity to name
    names: Vec<String>,

    /// the entities stored
    entities: Vec<E>,

    /// the map from id to component
    resources: Vec<T>,
}

impl<E: EntityTrait, T> Default for ResourceManager<E, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: EntityTrait, T> ResourceManager<E, T> {
    /// creates a new entity manager
    pub fn new() -> Self {
        let mut free_entity_queue = VecDeque::with_capacity(INITIAL_ENTITY_CAPACITY);
        for i in 0..INITIAL_ENTITY_CAPACITY {
            free_entity_queue.push_back(E::from_id(i as usize));
        }

        Self {
            free_entity_queue,
            name_to_entity_map: HashMap::new(),
            sparse: vec![usize::MAX; INITIAL_ENTITY_CAPACITY],
            entities: Vec::new(),
            names: Vec::new(),
            resources: Vec::new(),
        }
    }

    /// gets a new id
    fn get_new_entity(&mut self) -> E {
        if self.free_entity_queue.is_empty() {
            let new_count = (self.names.len() as f32 * ENTITY_CAPACITY_GROWTH_FACTOR)
                .min(usize::MAX as f32) as usize;
            for i in self.names.len() as usize..new_count {
                self.free_entity_queue.push_back(E::from_id(i));
            }
        }
        self.free_entity_queue.pop_front().unwrap()
    }

    /// gets the index of the given entity
    /// fails if the entity does not exist
    fn get_entity_index(&self, entity: E) -> Option<usize> {
        let idx = self.sparse[entity.id() as usize];
        if idx != usize::MAX { None } else { Some(idx) }
    }

    /// creates a new unnamed resource, returning the entity created
    pub fn add(&mut self, resource: T) -> E {
        let entity = self.get_new_entity();

        // expand sparse if id is too large
        if entity.id() >= self.sparse.len() as u32 {
            self.sparse.resize(entity.id() as usize + 1, usize::MAX);
        }

        let dense_idx = self.resources.len();
        self.sparse[entity.id() as usize] = dense_idx;

        self.entities.push(entity);
        self.resources.push(resource);
        self.names.push(String::new());

        entity
    }

    /// creates a new named resource, returning the entity created on success
    /// returns the resource on failure
    pub fn add_named(&mut self, name: String, resource: T) -> Result<E, T> {
        if name.is_empty() || self.name_to_entity_map.contains_key(&name) {
            return Err(resource);
        }

        let entity = self.get_new_entity();

        // expand sparse if id is too large
        if entity.id() >= self.sparse.len() as u32 {
            self.sparse.resize(entity.id() as usize + 1, usize::MAX);
        }

        let dense_idx = self.resources.len();
        self.sparse[entity.id() as usize] = dense_idx;

        self.entities.push(entity);
        self.resources.push(resource);
        self.names.push(name.clone());
        self.name_to_entity_map.insert(name, entity);

        Ok(entity)
    }

    /// removes a resource if it exists, returning it and its name
    pub fn remove(&mut self, entity: E) -> Option<(Option<String>, T)> {
        // Check if the entity actually has the component
        let remove_idx = if let Some(i) = self.get_entity_index(entity) {
            i
        } else {
            return None;
        };

        let last_dense_idx = self.resources.len() - 1;

        // Swap the target element with the last element in dense arrays
        self.entities.swap(remove_idx, last_dense_idx);
        self.resources.swap(remove_idx, last_dense_idx);
        self.names.swap(remove_idx, last_dense_idx);

        // Update sparse map for the entity that was moved into the new empty slot
        if remove_idx != last_dense_idx {
            let moved_entity = self.entities[remove_idx];
            self.sparse[moved_entity.id() as usize] = remove_idx;
        }

        // Pop the removed element from the end of the vectors
        self.sparse[entity.id() as usize] = usize::MAX;
        self.entities.pop();
        let resource = self.resources.pop().unwrap();
        let name = self.names.pop().unwrap();
        self.name_to_entity_map.remove(&name);

        let name = if name.is_empty() { None } else { Some(name) };

        Some((name, resource))
    }

    /// removes a resource by its name if it exists
    pub fn remove_by_name(&mut self, name: &String) -> Option<T> {
        if let Some(entity) = self.name_to_entity_map.get(name) {
            self.remove(*entity).map(|x| x.1)
        } else {
            None
        }
    }

    /// checks if a resource with the given entity exists
    pub fn contains(&self, entity: E) -> bool {
        self.get_entity_index(entity).is_some()
    }

    /// checks if a resource with the given name exists
    pub fn contains_name(&self, name: &String) -> bool {
        self.name_to_entity_map.contains_key(name)
    }

    /// gets a resource by its entity
    pub fn get(&self, entity: E) -> Option<&T> {
        self.resources.get(self.get_entity_index(entity)?)
    }

    /// gets a mutable resource by its entity
    pub fn get_mut(&mut self, entity: E) -> Option<&mut T> {
        let index = self.get_entity_index(entity)?;
        self.resources.get_mut(index)
    }

    /// gets a reference to the resource by name
    pub fn get_by_name(&self, name: &String) -> Option<&T> {
        self.get(*self.name_to_entity_map.get(name)?)
    }

    /// gets a mutable reference to the resource by name
    pub fn get_mut_by_name(&mut self, name: &String) -> Option<&mut T> {
        self.get_mut(*self.name_to_entity_map.get(name)?)
    }

    /// gets the name of the entity
    pub fn get_name(&self, entity: E) -> Option<&String> {
        Some(self.names.get(entity.id() as usize)?)
    }

    /// gets the entity by the given name
    pub fn get_entity(&self, name: &String) -> Option<E> {
        self.name_to_entity_map.get(name).cloned()
    }
}
