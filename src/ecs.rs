use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    u32,
};

use crate::{bitvec::BitSet, lua::FromLuaValue};

pub trait EntityTrait:
    Debug
    + Clone
    + Copy
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + Hash
    + Unpin
    + Send
    + Sync
    + Sized
    + FromLuaValue
{
    fn id(&self) -> u32;

    fn from_id(id: usize) -> Self;
}

impl<M> EntityTrait for Entity<M> {
    fn id(&self) -> u32 {
        self.0
    }

    fn from_id(id: usize) -> Self {
        Self(id as u32, PhantomData)
    }
}

impl<M> Debug for Entity<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Entity({:?}, {:?})", self.0, self.1)
    }
}

impl<M> FromLuaValue for Entity<M> {
    fn try_from_lua_value(value: &mlua::Value) -> Result<Self, String> {
        match value {
            mlua::Value::Number(float) => {
                if float.round() != *float {
                    Err(format!("Entity IDs must be integer values; got {float}"))
                } else if *float > u32::MAX as f64 || *float < u32::MIN as f64 {
                    Err(format!(
                        "Entity IDs must be in the range [{}, {}]; got {float}.",
                        u32::MIN,
                        u32::MAX
                    ))
                } else {
                    Ok(Self(*float as u32, PhantomData))
                }
            }

            mlua::Value::Integer(int) => {
                if let Ok(i) = <u32>::try_from(*int) {
                    Ok(Self(i, PhantomData))
                } else {
                    Err(format!(
                        "Entity IDs must be in the range [{}, {}]; got {int}.",
                        u32::MIN,
                        u32::MAX
                    ))
                }
            }

            _ => Err("Entity IDs must be integer values; got incorrect type.".to_string()),
        }
    }
}

pub const INITIAL_ENTITY_CAPACITY: usize = 1000;
pub const ENTITY_CAPACITY_GROWTH_FACTOR: f32 = 2.0;

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

/// efficiently maintains a set of entities
pub struct EntityManager<E: EntityTrait> {
    /// the list of available entity ids
    free_entity_queue: VecDeque<E>,

    /// the usable entity ids, including free ones
    entities: BitSet,
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
            entities: BitSet::with_capacity(INITIAL_ENTITY_CAPACITY),
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
        self.entities.test(entity.id() as usize)
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
#[derive(Debug)]
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

    /// gets a new entity
    fn get_new_entity(&mut self) -> E {
        if self.free_entity_queue.is_empty() {
            let new_count = (self.entity_names.len() as f32 * ENTITY_CAPACITY_GROWTH_FACTOR)
                .min(usize::MAX as f32) as usize;
            for i in self.entity_names.len() as usize..new_count {
                self.free_entity_queue.push_back(E::from_id(i));
            }
        }
        self.free_entity_queue.pop_front().unwrap()
    }

    /// gets a new nameless entity, O(1)
    pub fn get_new(&mut self) -> E {
        let entity = self.get_new_entity();
        self.entity_names.insert(entity, String::new());
        entity
    }

    /// creates an entity, assigning it a name, returns None if the name is in use
    /// empty string names are not allowed (returns None)
    pub fn get_new_named(&mut self, name: String) -> Option<E> {
        if name.is_empty() || self.name_to_entity_map.contains_key(&name) {
            return None;
        }

        let entity = self.get_new_entity();
        self.name_to_entity_map.insert(name.clone(), entity);
        self.entity_names.insert(entity, name);

        Some(entity)
    }

    /// gets the entity with the given name, if it exists, average O(1)
    pub fn get_by_name(&self, name: &String) -> Option<E> {
        self.name_to_entity_map.get(name).copied()
    }

    /// gets the name of the given entity, if it exists
    pub fn get_name(&self, entity: E) -> Option<&String> {
        self.entity_names.get(entity)
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

    /// returns an iterator over entities and their optional names
    pub fn iter(&self) -> impl Iterator<Item = (E, Option<&String>)> {
        self.entity_names.pairs_iter().map(|(e, n)| {
            let name = if n.is_empty() { None } else { Some(n) };
            (e, name)
        })
    }
}

/// manages a sparse set of entities, supporting fast lookup, insert, and deletion, but not iteration
#[derive(Debug, Clone)]
pub struct SparseSet<E: EntityTrait> {
    entities: BitSet,
    phantom: PhantomData<E>,
}

impl<E: EntityTrait> SparseSet<E> {
    /// creates a new dense storage
    pub fn new() -> Self {
        Self {
            entities: BitSet::with_capacity(INITIAL_ENTITY_CAPACITY),
            phantom: PhantomData,
        }
    }

    /// creates a new dense storage with the given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entities: BitSet::with_capacity(capacity),
            phantom: PhantomData,
        }
    }

    /// combine this set with another
    pub fn union(&mut self, other: Self) {
        self.entities.union(&other.entities);
    }

    /// returns the number of entities managed
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// whether this contains no entities/data
    pub fn is_empty(&self) -> bool {
        self.len() <= 0
    }

    /// inserts a component into this manager
    /// returns true if the entity was already contained
    pub fn insert(&mut self, entity: E) -> bool {
        let idx = entity.id() as usize;
        let out = self.entities.test(idx);
        self.entities.set(idx);
        out
    }

    /// removes an entity from this manager, returning true if the entity was contained
    pub fn remove(&mut self, entity: E) -> bool {
        let idx = entity.id() as usize;
        let out = self.entities.test(idx);
        self.entities.clear(idx);
        out
    }

    /// checks if the manager contains the given entity
    pub fn contains(&self, entity: E) -> bool {
        self.entities.test(entity.id() as usize)
    }
}

/// manages a set of entities supporting fast (but unordered) iteration over them
#[derive(Debug, Clone)]
pub struct DenseSet<E: EntityTrait> {
    entities: DenseComponentManager<E, ()>,
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
            entities: Default::default(),
        }
    }

    /// creates a new dense storage with the given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entities: DenseComponentManager::with_capacity(capacity),
        }
    }

    /// combine this set with another
    pub fn union(&mut self, other: Self) {
        for entity in other.entities.entities {
            self.insert(entity);
        }
    }

    /// returns the number of entities managed
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// whether this contains no entities/data
    pub fn is_empty(&self) -> bool {
        self.len() <= 0
    }

    /// inserts a component into this manager
    /// returns true if the entity was already contgained
    pub fn insert(&mut self, entity: E) -> bool {
        self.entities.insert(entity, ()).is_some()
    }

    /// removes an entity from this manager, returning true if the entity was contained
    pub fn remove(&mut self, entity: E) -> bool {
        self.entities.remove(entity).is_some()
    }

    /// checks if the manager contains the given entity
    pub fn contains(&self, entity: E) -> bool {
        self.entities.contains(entity)
    }

    /// returns an iterator over entities
    pub fn iter(&self) -> impl Iterator<Item = E> {
        self.entities.entity_iter()
    }
}

/// manages a single component for the board's ecs using a dense vector
#[derive(Debug, Clone)]
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

    /// creates a new dense storage with the given capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            sparse: vec![usize::MAX; capacity],
            entities: Vec::with_capacity(capacity),
            data: Vec::with_capacity(capacity),
        }
    }

    /// returns the number of entities managed
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// whether this contains no entities/data
    pub fn is_empty(&self) -> bool {
        self.len() <= 0
    }

    /// inserts a component into this manager
    /// returning the overwritten component if it exists
    pub fn insert(&mut self, entity: E, component: T) -> Option<T> {
        // expand sparse if id is too large
        if entity.id() >= self.sparse.len() as u32 {
            self.sparse.resize(entity.id() as usize + 1, usize::MAX);
        }

        // If the entity already has this component, overwrite it
        let idx = self.sparse[entity.id() as usize];
        if idx != usize::MAX {
            let mut output = component;
            std::mem::swap(&mut output, &mut self.data[idx]);
            return Some(output);
        }

        let dense_idx = self.data.len();
        self.entities.push(entity);
        self.data.push(component);
        self.sparse[entity.id() as usize] = dense_idx;

        None
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

    /// queries the manager for an entity, inserting a new component on failure
    pub fn get_or_insert(&mut self, entity: E, callback: impl FnOnce() -> T) -> &T {
        if !self.contains(entity) {
            self.insert(entity, callback());
        }

        self.get(entity).unwrap()
    }

    /// queries the manager for an entity, mutably
    pub fn get_mut(&mut self, entity: E) -> Option<&mut T> {
        let idx = *self.sparse.get(entity.id() as usize)?;
        if idx == usize::MAX {
            return None;
        }

        Some(&mut self.data[idx])
    }

    /// queries the manager for an entity, inserting a new component on failure
    pub fn get_mut_or_insert(&mut self, entity: E, callback: impl FnOnce() -> T) -> &mut T {
        if !self.contains(entity) {
            self.insert(entity, callback());
        }

        self.get_mut(entity).unwrap()
    }

    /// returns an iterator over component references
    pub fn component_iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    /// returns an iterator over mutable component references
    pub fn component_iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.data.iter_mut()
    }

    /// returns an iterator over entities
    pub fn entity_iter(&self) -> impl Iterator<Item = E> {
        self.entities.iter().copied()
    }

    /// returns an iterator over entities and their associated components
    pub fn pairs_iter(&self) -> impl Iterator<Item = (E, &T)> {
        self.entities.iter().copied().zip(self.data.iter())
    }

    /// returns an iterator over entities and their associated components mutably
    pub fn pairs_iter_mut(&mut self) -> impl Iterator<Item = (E, &mut T)> {
        self.entities.iter().copied().zip(self.data.iter_mut())
    }
}

/// efficiently maintains a set of entities potentially with associated names
/// entities can be accessed by their names
#[derive(Debug)]
pub struct ResourceManager<E: EntityTrait, T> {
    /// the list of available entity ids
    free_entity_queue: VecDeque<E>,

    /// the map from name to entity
    name_to_entity_map: HashMap<String, E>,

    /// the map from entity to index
    sparse: Vec<usize>,

    /// the map from index to name
    names: Vec<String>,

    /// the map from index to entities stored
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

    /// gets the number of entities stored
    pub fn len(&self) -> usize {
        self.entities.len()
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
        if idx == usize::MAX { None } else { Some(idx) }
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
        self.name_to_entity_map.insert(name.clone(), entity);

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
        self.name_to_entity_map.get(name).copied()
    }

    /// returns an iterator over all entities, names, and resources
    pub fn iter(&self) -> impl Iterator<Item = (E, Option<&String>, &T)> {
        self.entities
            .iter()
            .zip(self.names.iter())
            .zip(self.resources.iter())
            .map(|((e, n), r)| {
                let name = if n.is_empty() { None } else { Some(n) };
                (*e, name, r)
            })
    }

    /// returns an iterator over entities and their names
    pub fn identifier_iter(&self) -> impl Iterator<Item = (E, Option<&String>)> {
        self.entities.iter().zip(self.names.iter()).map(|(e, n)| {
            let name = if n.is_empty() { None } else { Some(n) };
            (*e, name)
        })
    }

    /// returns an iterator over all resources
    pub fn resource_iter(&self) -> impl Iterator<Item = &T> {
        self.resources.iter()
    }

    /// returns a mutable iterator over all resources
    pub fn resource_iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.resources.iter_mut()
    }
}

/// like a normal resource manager, but counts resource references
/// unnamed resources are removed automatically upon reference count reaching 0
pub struct RcResourceManager<E: EntityTrait, T> {
    /// the list of available entity ids
    free_entity_queue: VecDeque<E>,

    /// the map from name to entity
    name_to_entity_map: HashMap<String, E>,

    /// the map from entity to index
    sparse: Vec<usize>,

    /// the map from index to name
    names: Vec<String>,

    /// the map from index to entities stored
    entities: Vec<E>,

    /// the map from id to component
    resources: Vec<T>,

    /// the map from id to reference count
    ref_counts: Vec<usize>,
}

impl<E: EntityTrait, T> Default for RcResourceManager<E, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: EntityTrait, T> RcResourceManager<E, T> {
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
            ref_counts: Vec::new(),
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
        if idx == usize::MAX { None } else { Some(idx) }
    }

    /// creates a new unnamed resource, returning the entity created
    /// this operation does not increase reference count for this entity, you must do that manually
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
        self.ref_counts.push(0);

        entity
    }

    /// creates a new named resource, returning the entity created on success
    /// returns the resource on failure
    /// this operation does not increase reference count for this entity, you must do that manually
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
        self.ref_counts.push(0);

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
        self.ref_counts.swap(remove_idx, last_dense_idx);

        // Update sparse map for the entity that was moved into the new empty slot
        if remove_idx != last_dense_idx {
            let moved_entity = self.entities[remove_idx];
            self.sparse[moved_entity.id() as usize] = remove_idx;
        }

        // Pop the removed element from the end of the vectors
        self.sparse[entity.id() as usize] = usize::MAX;
        self.entities.pop();
        self.ref_counts.pop();
        let resource = self.resources.pop().unwrap();
        let name = self.names.pop().unwrap();
        self.name_to_entity_map.remove(&name);

        let name = if name.is_empty() { None } else { Some(name) };

        Some((name, resource))
    }

    /// removes a resource by its name
    pub fn remove_by_name(&mut self, name: &String) -> Option<T> {
        if let Some(entity) = self.name_to_entity_map.get(name) {
            self.remove(*entity).map(|x| x.1)
        } else {
            None
        }
    }

    /// gets the reference count of the given entity
    pub fn get_references(&self, entity: E) -> Option<usize> {
        Some(self.ref_counts[self.get_entity_index(entity)?])
    }

    /// increments the reference count for the given entity
    /// returns false if the entity does not exist
    pub fn increment_references(&mut self, entity: E) -> bool {
        if let Some(idx) = self.get_entity_index(entity) {
            self.ref_counts[idx] += 1;
            true
        } else {
            false
        }
    }

    /// decrements the reference count for the given entity
    /// returns the removed resource, if reference count reached 0
    pub fn decrement_references(&mut self, entity: E) -> Option<(Option<String>, T)> {
        if let Some(idx) = self.get_entity_index(entity) {
            self.ref_counts[idx] = self.ref_counts[idx].saturating_sub(1);

            if self.ref_counts[idx] <= 0 && self.names[idx].is_empty() {
                self.remove(entity)
            } else {
                None
            }
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
        self.name_to_entity_map.get(name).copied()
    }

    /// returns an iterator over all entities, names, and resources
    pub fn iter(&self) -> impl Iterator<Item = (E, Option<&String>, &T)> {
        self.entities
            .iter()
            .zip(self.names.iter())
            .zip(self.resources.iter())
            .map(|((e, n), r)| {
                let name = if n.is_empty() { None } else { Some(n) };
                (*e, name, r)
            })
    }

    /// returns an iterator over entities and their names
    pub fn identifier_iter(&self) -> impl Iterator<Item = (E, Option<&String>)> {
        self.entities.iter().zip(self.names.iter()).map(|(e, n)| {
            let name = if n.is_empty() { None } else { Some(n) };
            (*e, name)
        })
    }

    /// returns an iterator over all resources
    pub fn resource_iter(&self) -> impl Iterator<Item = &T> {
        self.resources.iter()
    }

    /// returns a mutable iterator over all resources
    pub fn resource_iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.resources.iter_mut()
    }
}
