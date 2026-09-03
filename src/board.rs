use std::collections::{BTreeMap, VecDeque};

use mlua::{Table, Value};

use crate::{
    ecs::{
        DenseComponentManager, DenseSet, Entity, EntityTrait, NamedEntityManager, ResourceManager,
    },
    geometry::{self, CollisionShape, IConvexShape, IRect, IVec2, Transform},
    lua::{FromLuaTable, FromLuaValue, vector_from_lua_value},
    quadtree::QuadTree,
    resources::{TextureEntity, TextureManager},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceEntityMarker;
pub type PieceEntity = Entity<PieceEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionShapeEntityMarker;
pub type CollisionShapeEntity = Entity<CollisionShapeEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionLayerEntityMarker;
pub type CollisionLayerEntity = Entity<CollisionLayerEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceCategoryEntityMarker;
pub type CategoryEntity = Entity<PieceCategoryEntityMarker>;

#[derive(Debug, Clone)]
pub struct PieceCollider {
    /// the collision shape entity
    pub shape: CollisionShapeEntity,

    /// the collision layers this collider "cares about"
    pub layer: CollisionLayerEntity,
}

#[derive(Debug, Clone)]
pub struct StackInfo<V: IVec2> {
    /// the pieces on this stack from deepest to topmost
    pub pieces: Vec<PieceEntity>,

    /// the offset of each piece from the bottom most piece
    pub offset: V,

    /// the radius of the snapping cirle
    pub snap_radius: f32,
}

/// an identifier for an entity, which may either be by name or entity id
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityIdentifier<E: EntityTrait> {
    Entity(E),
    Name(String),
}

impl<E: EntityTrait> FromLuaValue for EntityIdentifier<E> {
    fn try_from_lua_value(value: &mlua::Value) -> Result<Self, String> {
        if let mlua::Value::String(s) = value {
            Ok(Self::Name(format!("{}", s.display())))
        } else {
            Ok(Self::Entity(E::try_from_lua_value(value)?))
        }
    }
}

/// collision shape data intermediary between lua tables and actual collision shape
#[derive(Debug, Clone)]
pub enum CollisionShapeIR<S: IConvexShape + FromLuaTable> {
    Identifier(EntityIdentifier<CollisionShapeEntity>),
    Instance(CollisionShape<S>),
}

impl<S: IConvexShape + FromLuaTable> FromLuaValue for CollisionShapeIR<S> {
    fn try_from_lua_value(value: &mlua::Value) -> Result<Self, String> {
        if let mlua::Value::Table(table) = value {
            Ok(Self::Instance(CollisionShape::try_from_lua_table(table)?))
        } else {
            Ok(Self::Identifier(EntityIdentifier::try_from_lua_value(
                value,
            )?))
        }
    }
}

/// collider data that serves as an intermediary between lua tables and a pieces collider
#[derive(Debug, Clone)]
pub struct PieceColliderIR<S: IConvexShape + FromLuaTable> {
    /// the collision shape entity
    pub shape: CollisionShapeIR<S>,

    /// the collision layer this collider belongs on
    pub layer: String,
}

impl<S: IConvexShape + FromLuaTable> FromLuaValue for PieceColliderIR<S> {
    fn try_from_lua_value(value: &mlua::prelude::LuaValue) -> Result<Self, String> {
        match value {
            Value::Table(table) => Self::try_from_lua_table(&table),
            _ => Err(format!("non-table value.")),
        }
    }
}

impl<S: IConvexShape + FromLuaTable> FromLuaTable for PieceColliderIR<S> {
    fn try_from_lua_table(table: &mlua::prelude::LuaTable) -> Result<Self, String> {
        let shape = if let Ok(shape_table) = table.raw_get::<Value>("shape") {
            match CollisionShapeIR::try_from_lua_value(&shape_table) {
                Ok(shape) => shape,
                Err(msg) => return Err(format!("Invalid shape field: {msg}")),
            }
        } else {
            return Err(format!("Invalid shape field."));
        };

        let layer = if let Ok(value) = table.raw_get::<Value>("layer") {
            match String::try_from_lua_value(&value) {
                Ok(v) => v,
                Err(msg) => return Err(format!("Invalid layer field: {msg}")),
            }
        } else {
            return Err(format!("Invalid layer field."));
        };

        Ok(Self { shape, layer })
    }
}

/// piece data that serves as an intermediary between lua tables an a piece on the board
pub struct PieceIR<S: IConvexShape + FromLuaTable, V: IVec2> {
    /// optional name
    pub name: Option<String>,

    /// optional texture (invisible pieces are allowed for gameplay reasons)
    pub texture: Option<EntityIdentifier<TextureEntity>>,

    /// the transform for the piece
    pub transform: Transform<V>,

    /// colliders
    pub colliders: Vec<PieceColliderIR<S>>,

    /// draw layer
    pub draw_layer: i32,

    /// categorizations
    pub categories: Vec<String>,
}

impl<S: IConvexShape + FromLuaTable, V: IVec2> FromLuaValue for PieceIR<S, V> {
    fn try_from_lua_value(value: &mlua::prelude::LuaValue) -> Result<Self, String> {
        match value {
            Value::Table(table) => Self::try_from_lua_table(&table),
            _ => Err(format!("non-table value.")),
        }
    }
}

impl<S: IConvexShape + FromLuaTable, V: IVec2> FromLuaTable for PieceIR<S, V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let name = if let Ok(val) = table.raw_get::<Value>("name") {
            if let Value::Nil = val {
                None
            } else {
                match String::try_from_lua_value(&val) {
                    Ok(t) => Some(t),
                    Err(msg) => return Err(format!("Invalid name field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid name field."));
        };

        let texture = if let Ok(val) = table.raw_get::<Value>("texture") {
            if let Value::Nil = val {
                None
            } else {
                match EntityIdentifier::try_from_lua_value(&val) {
                    Ok(t) => Some(t),
                    Err(msg) => return Err(format!("Invalid texture field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid texture field."));
        };

        let transform = if let Ok(val) = table.raw_get::<Value>("transform") {
            if let Value::Nil = val {
                Default::default()
            } else {
                match Transform::try_from_lua_value(&val) {
                    Ok(t) => t,
                    Err(msg) => return Err(format!("Invalid transform field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid transform field."));
        };

        let colliders = if let Ok(value) = table.raw_get::<Value>("colliders") {
            vector_from_lua_value(&value, "collider field", "collider")?
        } else {
            return Err(format!("Invalid collider field."));
        };

        let draw_layer = if let Ok(value) = table.raw_get::<Value>("draw_layer") {
            match &value {
                Value::Nil => 0,
                Value::Number(float) => {
                    if float.round() != *float {
                        return Err(format!("Invalid draw_layer field: non-integer value."));
                    }
                    if float.clamp(i32::MIN as f64, i32::MAX as f64) != *float {
                        return Err(format!(
                            "Invalid draw_layer field: must be in range [{}, {}].",
                            i32::MIN,
                            i32::MAX
                        ));
                    }
                    *float as i32
                }
                Value::Integer(int) => {
                    if (*int).clamp(i32::MIN as i64, i32::MAX as i64) != *int {
                        return Err(format!(
                            "Invalid draw_layer field: must be in range [{}, {}].",
                            i32::MIN,
                            i32::MAX
                        ));
                    }
                    *int as i32
                }
                _ => return Err(format!("Invalid draw_layer field: non-integer value.")),
            }
        } else {
            return Err(format!("Invalid draw_layer field."));
        };

        let categories = if let Ok(value) = table.raw_get::<Value>("categories") {
            vector_from_lua_value(&value, "categories field", "category")?
        } else {
            return Err(format!("Invalid categories field."));
        };

        Ok(Self {
            name,
            texture,
            transform,
            colliders,
            draw_layer,
            categories,
        })
    }
}

struct CategoryManager {
    /// the categories of pieces that may exist
    /// maps categories to the list of pieces in that category
    /// we must ensure that all categories are named entities
    category_collections: ResourceManager<CategoryEntity, DenseSet<PieceEntity>>,

    /// the categories assigned to each piece
    piece_categories: DenseComponentManager<PieceEntity, DenseSet<CategoryEntity>>,
}

impl Default for CategoryManager {
    fn default() -> Self {
        Self {
            category_collections: Default::default(),
            piece_categories: Default::default(),
        }
    }
}

impl CategoryManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// adds categories to a piece
    pub fn add_categories(&mut self, piece: PieceEntity, categories: &[String]) {
        let piece_categories = if let Some(val) = self.piece_categories.get_mut(piece) {
            val
        } else {
            self.piece_categories.insert(piece, DenseSet::new());
            self.piece_categories.get_mut(piece).unwrap()
        };

        for category in categories.iter() {
            let (category_entity, category_collection) =
                if let Some(e) = self.category_collections.get_entity(&category) {
                    (e, self.category_collections.get_mut(e).unwrap())
                } else {
                    let entity = self
                        .category_collections
                        .add_named(category.clone(), DenseSet::new())
                        .unwrap();
                    (entity, self.category_collections.get_mut(entity).unwrap())
                };

            piece_categories.insert(category_entity);
            category_collection.insert(piece);
        }
    }

    /// removes categories from a piece
    pub fn remove_categories(&mut self, piece: PieceEntity, categories: &[String]) {
        let piece_categories = if let Some(val) = self.piece_categories.get_mut(piece) {
            val
        } else {
            self.piece_categories.insert(piece, DenseSet::new());
            self.piece_categories.get_mut(piece).unwrap()
        };

        for category in categories.iter() {
            if let Some(category_entity) = self.category_collections.get_entity(&category) {
                let category_collection =
                    self.category_collections.get_mut(category_entity).unwrap();

                piece_categories.remove(category_entity);
                category_collection.remove(piece);

                if category_collection.is_empty() {
                    self.category_collections.remove(category_entity);
                }
            }
        }
    }

    /// returns an iterator over all existing categories
    pub fn categories(&self) -> impl Iterator<Item = &String> {
        self.category_collections
            .identifier_iter()
            .map(|(_, n)| n.unwrap())
    }

    /// returns an iterator over all categories assigned to a piece
    pub fn piece_categories(&self, piece: PieceEntity) -> Option<impl Iterator<Item = &String>> {
        let categories = self.piece_categories.get(piece)?;
        Some(
            categories
                .iter()
                .map(|e| self.category_collections.get_name(e).unwrap()),
        )
    }

    /// returns an iterator over all pieces in a category
    pub fn category_pieces(&self, category: &String) -> Option<impl Iterator<Item = PieceEntity>> {
        if let Some(collection) = self.category_collections.get_by_name(category) {
            Some(collection.iter())
        } else {
            None
        }
    }
}

struct DrawLayerManager {
    /// ref counts for draw layers
    /// allows linear time construction of render layers during render operations
    draw_layer_refs: BTreeMap<i32, usize>,

    /// the draw layers of each piece
    piece_draw_layers: DenseComponentManager<PieceEntity, i32>,
}

impl Default for DrawLayerManager {
    fn default() -> Self {
        Self {
            draw_layer_refs: Default::default(),
            piece_draw_layers: Default::default(),
        }
    }
}

impl DrawLayerManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn decrement_ref_count(draw_layer_refs: &mut BTreeMap<i32, usize>, layer: i32) {
        if let Some(refs) = draw_layer_refs.get_mut(&layer) {
            *refs = refs.saturating_sub(1);
            if *refs <= 0 {
                draw_layer_refs.remove(&layer);
            }
        }
    }

    fn increment_ref_count(draw_layer_refs: &mut BTreeMap<i32, usize>, layer: i32) {
        let refs = draw_layer_refs.entry(layer).or_insert(0);
        *refs += 1;
    }

    pub fn remove_piece(&mut self, entity: PieceEntity) {
        if let Some(layer) = self.piece_draw_layers.remove(entity) {
            Self::decrement_ref_count(&mut self.draw_layer_refs, layer);
        }
    }

    pub fn set(&mut self, entity: PieceEntity, layer: i32) {
        // handle case where piece already exists in this manager
        if let Some(old_layer) = self.piece_draw_layers.get_mut(entity) {
            // if no layer change occurs do nothing
            if *old_layer == layer {
                return;
            }

            // update refcounts and values
            Self::decrement_ref_count(&mut self.draw_layer_refs, *old_layer);
            *old_layer = layer;
            Self::increment_ref_count(&mut self.draw_layer_refs, layer);
        }
    }

    pub fn get(&self, entity: PieceEntity) -> Option<i32> {
        self.piece_draw_layers.get(entity).copied()
    }

    pub fn layer_iter(&self) -> impl Iterator<Item = i32> {
        self.draw_layer_refs.keys().copied()
    }
}

struct CollisionLayerManager {
    layers: ResourceManager<CollisionLayerEntity, usize>,
}

impl Default for CollisionLayerManager {
    fn default() -> Self {
        Self {
            layers: Default::default(),
        }
    }
}

impl CollisionLayerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// increments the ref count for the layer with the given name
    /// returns the corresponding layer entity
    pub fn increment_count(&mut self, layer: &String) -> CollisionLayerEntity {
        if let Some(entity) = self.layers.get_entity(layer) {
            *self.layers.get_mut(entity).unwrap() += 1;
            entity
        } else {
            self.layers.add_named(layer.clone(), 1).unwrap()
        }
    }

    /// gets the entity corresponding to the layer
    pub fn get_entity(&self, layer: &String) -> Option<CollisionLayerEntity> {
        self.layers.get_entity(layer)
    }

    /// decrements the ref count for the layer
    pub fn decrement_count(&mut self, layer: CollisionLayerEntity) {
        if let Some(count) = self.layers.get_mut(layer) {
            *count -= 1;

            if *count <= 0 {
                self.layers.remove(layer);
            }
        }
    }

    /// returns an iterator over all stored collision layers
    pub fn layer_iter(&self) -> impl Iterator<Item = &String> {
        self.layers.identifier_iter().map(|(_, n)| n.unwrap())
    }
}

pub struct QuadTreeManager<R: IRect> {
    /// aabbs for each piece
    aabbs: DenseComponentManager<PieceEntity, R>,

    /// the quadtree used to track collisions
    quadtree: QuadTree<PieceEntity, R>,
}

impl<R: IRect> QuadTreeManager<R> {
    pub fn new(quadtree_rect: R) -> Self {
        Self {
            aabbs: Default::default(),
            quadtree: QuadTree::new(quadtree_rect),
        }
    }

    /// removes the piece from this quadtree, returning its aabb if its
    pub fn remove(&mut self, piece: PieceEntity) -> Option<R> {
        let old_aabb = self.aabbs.remove(piece)?;
        self.quadtree
            .extract_from_regions_on_point(old_aabb.center::<(f32, f32)>(), &|e| *e == piece);
        Some(old_aabb)
    }

    /// sets the aabb for the given piece
    pub fn set_aabb(&mut self, piece: PieceEntity, aabb: R) {
        if let Some(old_aabb) = self.aabbs.get_mut(piece) {
            // update and remove old aabb from quadtree
            self.quadtree
                .extract_from_regions_on_point(old_aabb.center::<(f32, f32)>(), &|e| *e == piece);
            *old_aabb = aabb;
        } else {
            self.aabbs.insert(piece, aabb.clone());
        }

        // insert new aabb into quadtree
        self.quadtree
            .insert(piece, &|e| *self.aabbs.get(*e).unwrap());
    }

    /// gets the rectangle the quadtree occupies
    pub fn rect(&self) -> R {
        self.quadtree.aabb()
    }
}

pub struct BoardManager<R: IRect, V: IVec2, S: IConvexShape + FromLuaTable, T, F> {
    /// textures resources that can be used
    /// unnamed textures must be uniquely owned
    texture_resources: TextureManager<T, F>,

    /// collision shape resources that can be referenced by pieces
    /// unnamed collision shapes must be uniquely owned
    collision_shape_resources: ResourceManager<CollisionShapeEntity, CollisionShape<S>>,

    /// a list of existing collision layers
    /// we will ensure that collision layers always have names
    collision_layers: CollisionLayerManager,

    /// the draw layers of pieces
    draw_layers: DrawLayerManager,

    /// the categories of pieces
    categories: CategoryManager,

    /// the entity manager for pieces and stacks
    piece_entity_manager: NamedEntityManager<PieceEntity>,

    /// stacks of pieces
    /// if an entity is in this set it may not have the following components
    ///  - piece_textures
    ///  - draw_layers
    ///  - colliders
    /// if pieces are added to a stack
    ///  - you cannot move them off of the stack
    ///  - all of their colliders will be used to compute the stack's aabb
    ///  - the aabbs of each piece will be removed from the quadtree (for performance reasons)
    stacks: DenseComponentManager<PieceEntity, StackInfo<V>>,

    /// the textures for each piece
    piece_textures: DenseComponentManager<PieceEntity, TextureEntity>,

    /// the order in which to draw entities
    /// all piece entities are found in this vector, even if they do not have a texture
    /// this is so the draw order of pieces is stable even after adding or removing their
    /// texture component
    draw_order: VecDeque<PieceEntity>,

    /// the transforms of each piece
    piece_transforms: DenseComponentManager<PieceEntity, Transform<V>>,

    /// the colliders for each piece
    /// we must ensure that each piece collider vector is non-emtpy
    piece_colliders: DenseComponentManager<PieceEntity, Vec<PieceCollider>>,

    /// quadtree manager that tracks piece aabb collisions
    /// also tracks the board's dimensions
    quadtree: QuadTreeManager<R>,
}

impl<R: IRect, V: IVec2, S: IConvexShape + FromLuaTable, T, F> BoardManager<R, V, S, T, F> {
    pub fn validate_shape_ir(&self, ir: &CollisionShapeIR<S>) -> Result<(), String> {
        if let CollisionShapeIR::Identifier(i) = ir {
            match &i {
                EntityIdentifier::Entity(e) => {
                    if !self.collision_shape_resources.contains(*e) {
                        return Err(format!("No collision shape with id {} exists.", e.id()));
                    }
                }
                EntityIdentifier::Name(n) => {
                    if !self.collision_shape_resources.contains_name(n) {
                        return Err(format!("No collision shape with name {n} exists."));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn validate_collider_ir(&self, ir: &PieceColliderIR<S>) -> Result<(), String> {
        self.validate_shape_ir(&ir.shape)?;

        Ok(())
    }

    pub fn validate_piece_ir(&self, ir: &PieceIR<S, V>) -> Result<(), String> {
        if let Some(name) = &ir.name
            && self.piece_entity_manager.is_name_used(&name)
        {
            return Err(format!("A piece named '{name}' already exists."));
        }

        if let Some(texture) = &ir.texture {
            match &texture {
                EntityIdentifier::Entity(e) => {
                    if !self.texture_resources.contains_texture(*e) {
                        return Err(format!("No texture with id {} exists.", e.id()));
                    }
                }
                EntityIdentifier::Name(n) => {
                    if !self.texture_resources.contains_texture_name(n) {
                        return Err(format!("No texture with name {n} exists."));
                    }
                }
            }
        }

        for collider in &ir.colliders {
            self.validate_collider_ir(collider)?;
        }

        Ok(())
    }

    /// check if a piece or stack entity is valid for this manager
    /// if so, returns the resolved entity
    fn is_entity_valid(&self, piece: EntityIdentifier<PieceEntity>) -> Result<PieceEntity, String> {
        match piece {
            EntityIdentifier::Entity(e) => {
                if !self.piece_entity_manager.is_used(e) {
                    Err(format!("Piece ID {} is not in use.", e.id()))
                } else {
                    Ok(e)
                }
            }

            EntityIdentifier::Name(n) => {
                if let Some(e) = self.piece_entity_manager.get_by_name(&n) {
                    Ok(e)
                } else {
                    Err(format!("Piece with name {n} does not exist."))
                }
            }
        }
    }

    /// check if a piece entity is valid for this manager
    /// if so, returns the resolved entity
    /// returns an error if the entity is not tracked or not a piece (may be a stack)
    pub fn is_piece_valid(
        &self,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<PieceEntity, String> {
        let entity = self.is_entity_valid(piece)?;

        if self.stacks.contains(entity) {
            if let Some(n) = self.piece_entity_manager.get_name(entity) {
                Err(format!(
                    "ID {} ('{n}') corresponds to a stack, not a piece.",
                    entity.id()
                ))
            } else {
                Err(format!(
                    "ID {} corresponds to a stack, not a piece.",
                    entity.id()
                ))
            }
        } else {
            Ok(entity)
        }
    }

    /// checks if a texture is valid
    pub fn is_texture_valid(&self, texture: EntityIdentifier<TextureEntity>) -> Result<(), String> {
        match texture {
            EntityIdentifier::Entity(e) => {
                if !self.texture_resources.contains_texture(e) {
                    return Err(format!("No texture has ID {}.", e.id()));
                }
            }

            EntityIdentifier::Name(n) => {
                if !self.texture_resources.contains_texture_name(&n) {
                    return Err(format!("No texture has name {n}."));
                }
            }
        }

        Ok(())
    }

    /// sets the texture of a piece without checking whether the or texture is valid
    pub fn set_piece_texture(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        texture: Option<EntityIdentifier<TextureEntity>>,
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece.clone())?;
        if let Some(t) = &texture {
            self.is_texture_valid(t.clone())?;
        }

        self.set_piece_texture_unchecked(piece, texture);
        Ok(())
    }

    /// sets the texture of a piece without checking whether the or texture is valid
    fn set_piece_texture_unchecked(
        &mut self,
        piece: PieceEntity,
        texture: Option<EntityIdentifier<TextureEntity>>,
    ) {
        if let Some(identifier) = &texture {
            // change to a texture

            // determine texture entity
            let new_texture = match identifier {
                EntityIdentifier::Entity(e) => *e,
                EntityIdentifier::Name(name) => {
                    self.texture_resources.get_texture_entity(&name).unwrap()
                }
            };

            // modify texture
            if let Some(old_texture) = self.piece_textures.get_mut(piece) {
                // change texture
                *old_texture = new_texture;
            } else {
                // add new texture
                self.piece_textures.insert(piece, new_texture);
            };
        } else {
            // remove texture
            self.piece_textures.remove(piece).is_some();
        }
    }

    /// sets the piece's transform
    pub fn set_piece_transform(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        transform: Transform<V>,
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.set_piece_transform_unchecked(piece, transform);
        Ok(())
    }

    /// sets the transform of a piece without checking whether the piece or transform is valid
    fn set_piece_transform_unchecked(&mut self, piece: PieceEntity, transform: Transform<V>) {
        // clamp transform so that its origin fits in the board
        let transform = Transform {
            offset: self.quadtree.rect().fit_point(transform.offset),
            angle: transform.angle,
        };

        // modify transforms
        if let Some(old_transform) = self.piece_transforms.get_mut(piece) {
            *old_transform = transform;
        } else {
            self.piece_transforms.insert(piece, transform.clone());
        }

        // update the quadtree if necessary
        if let Some(colliders) = self.piece_colliders.get(piece) {
            let initial_aabb: R = geometry::compute_advanced_aabb(
                &transform,
                colliders
                    .iter()
                    .map(|c| self.collision_shape_resources.get(c.shape).unwrap()),
            )
            .unwrap();

            // ensure colliders fit into the tree
            let final_aabb: R = self.quadtree.rect().fit(initial_aabb);
            let offset = final_aabb.min::<V>().sub(initial_aabb.min::<V>());
            if offset != V::from_xy(0.0, 0.0) {
                let transform = self.piece_transforms.get_mut(piece).unwrap();
                transform.offset = transform.offset.add(offset);
            }

            self.quadtree.set_aabb(piece, final_aabb);
        }
    }

    /// sets the colliders of the pice
    fn set_piece_colliders(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        collider_irs: Vec<PieceColliderIR<S>>,
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;

        for collider in &collider_irs {
            self.validate_collider_ir(collider)?;
        }

        self.set_piece_colliders_unchecked(piece, collider_irs);
        Ok(())
    }

    /// sets the colliders of a piece without checking whether the piece or collider list is valid
    /// (silently "fails") upon receiving an empty vector of colliders
    fn set_piece_colliders_unchecked(
        &mut self,
        piece: PieceEntity,
        collider_irs: Vec<PieceColliderIR<S>>,
    ) {
        let mut colliders = Vec::new();
        for collider_ir in collider_irs {
            let shape_entity = match collider_ir.shape {
                CollisionShapeIR::Identifier(i) => match i {
                    EntityIdentifier::Entity(e) => e,
                    EntityIdentifier::Name(n) => {
                        self.collision_shape_resources.get_entity(&n).unwrap()
                    }
                },
                CollisionShapeIR::Instance(val) => self.collision_shape_resources.add(val),
            };

            let layer_entity = self.collision_layers.increment_count(&collider_ir.layer);

            let collider = PieceCollider {
                shape: shape_entity,
                layer: layer_entity,
            };
            colliders.push(collider);
        }
        if !colliders.is_empty() {
            self.piece_colliders.insert(piece, colliders);
        }
    }

    pub fn add_piece(&mut self, ir: PieceIR<S, V>) -> Result<PieceEntity, String> {
        self.validate_piece_ir(&ir)?;

        let entity = if let Some(name) = &ir.name {
            self.piece_entity_manager
                .get_new_named(name.clone())
                .unwrap()
        } else {
            self.piece_entity_manager.get_new()
        };

        self.set_piece_texture_unchecked(entity, ir.texture);

        self.draw_layers.set(entity, ir.draw_layer);

        self.categories.add_categories(entity, &ir.categories);

        self.draw_order.push_back(entity);

        self.set_piece_colliders_unchecked(entity, ir.colliders);

        self.set_piece_transform_unchecked(entity, ir.transform);

        Ok(entity)
    }

    /// returns an iterator over the existing draw layers in sorted order
    pub fn draw_layer_iter(&self) -> impl Iterator<Item = i32> {
        self.draw_layers.layer_iter()
    }

    /// returns an iterator over texture-owning pieces and their transforms in the order in which
    /// they should be drawn (appearing first means should be on the bottom)
    pub fn texture_draw_iter(&self) -> impl Iterator<Item = (&T, Transform<V>, i32)> {
        self.draw_order.iter().filter_map(|m| {
            let texture_entity = self.piece_textures.get(*m)?;
            let texture = &self
                .texture_resources
                .get_texture(*texture_entity)
                .unwrap()
                .texture;
            let transform = self.piece_transforms.get(*m).unwrap();
            let draw_layer = self.draw_layers.get(*m).unwrap();

            Some((texture, *transform, draw_layer))
        })
    }
}
