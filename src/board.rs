use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mlua::{Table, Value};

use crate::{
    ecs::{
        DenseComponentManager, DenseSet, Entity, EntityTrait, NamedEntityManager, RcResourceManager, ResourceManager
    },
    geometry::{CollisionShape, IConvexShape, IRect, IVec2, Transform},
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
    /// the identifier for this collider
    pub name: String,

    /// the collision shape entity
    pub shape: PieceEntity,

    /// the collision layers this collider "cares about"
    pub layers: Vec<CollisionLayerEntity>,
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
    /// the identifier for this collider
    pub name: String,

    /// the collision shape entity
    pub shape: CollisionShapeIR<S>,

    /// the collision layers this collider "cares about"
    pub layers: Vec<EntityIdentifier<CollisionLayerEntity>>,
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
        let name = if let Ok(n) = table.raw_get::<String>("name") {
            n
        } else {
            return Err(format!("Invalid name field."));
        };

        let shape = if let Ok(shape_table) = table.raw_get::<Value>("shape") {
            match CollisionShapeIR::try_from_lua_value(&shape_table) {
                Ok(shape) => shape,
                Err(msg) => return Err(format!("Invalid shape field: {msg}")),
            }
        } else {
            return Err(format!("Invalid shape field."));
        };

        let layers = if let Ok(value) = table.raw_get::<Value>("layers") {
            vector_from_lua_value(&value, "layer field", "layer")?
        } else {
            return Err(format!("Invalid layers field."));
        };

        Ok(Self {
            name,
            shape,
            layers,
        })
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

pub struct BoardManager<R: IRect, V: IVec2, S: IConvexShape + FromLuaTable, T, F> {
    /// textures resources that can be used
    texture_resources: TextureManager<T, F>,

    // ----------------------------------------------------------------------------------
    /// collision shape resources that can be referenced by pieces
    collision_shape_resources: ResourceManager<CollisionShapeEntity, CollisionShape<S>>,

    /// a list of existing collision layers
    collision_layers: RcResourceManager<CollisionLayerEntity, ()>,

    // ----------------------------------------------------------------------------------
    /// the draw layers that exist and the number of pieces in the layer
    draw_layer_collections: BTreeMap<i32, usize>,

    /// the draw layers of each piece
    piece_draw_layers: DenseComponentManager<PieceEntity, i32>,

    // ----------------------------------------------------------------------------------
    // ----------------------------------------------------------------------------------
    /// the categories of pieces that may exist
    /// maps categories to the list of pieces in that category
    /// we must ensure that all categories are named entities
    piece_category_collections: RcResourceManager<CategoryEntity, DenseSet<PieceEntity>>,

    /// the categories assigned to each piece
    piece_categories: DenseComponentManager<PieceEntity, DenseSet<CategoryEntity>>,

    // ----------------------------------------------------------------------------------
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

    /// the transforms of each piece
    piece_transforms: DenseComponentManager<PieceEntity, Transform<V>>,

    /// the colliders for each piece
    piece_colliders: DenseComponentManager<PieceEntity, Vec<PieceCollider>>,

    /// neat aabbs for each piece
    /// aabbs may be removed when pieces get picked up
    piece_aabbs: DenseComponentManager<PieceEntity, R>,

    /// the order in which to draw entities,
    /// adding/removing a texture adds/removes an entity from this list
    /// picking up an entity places it on the top of this list
    draw_order: VecDeque<PieceEntity>,

    /// the quadtree used to track collisions
    quadtree: QuadTree<PieceEntity, R>,
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

        for layer in &ir.layers {
            if let EntityIdentifier::Entity(e) = layer
                && !self.collision_layers.contains(*e)
            {
                return Err(format!("No layer with id {} exists.", e.id()));
            }
        }

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

        let mut collider_name_set: BTreeSet<&str> = BTreeSet::new();
        for collider in &ir.colliders {
            let name: &str = &collider.name;
            if collider_name_set.contains(name) {
                return Err(format!("Duplicate collider name: {}.", collider.name));
            }
            collider_name_set.insert(name);

            self.validate_collider_ir(collider)?
        }

        Ok(())
    }

    pub fn add_piece(&mut self, ir: &PieceIR<S, V>) -> Result<PieceEntity, String> {
        self.validate_piece_ir(ir)?;

        todo!()
    }
}
