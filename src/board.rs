use std::{
    collections::{BTreeMap, VecDeque},
    fmt::Display,
    path::Path,
};

use mlua::{Lua, Table, Value};

use crate::{
    color::IColor,
    ecs::{
        DenseComponentManager, DenseSet, Entity, EntityTrait, NamedEntityManager, ResourceManager,
    },
    geometry::{self, CollisionShape, IConvexShape, IRect, IVec2, Transform},
    lua::{FromLuaTable, FromLuaValue, vector_from_lua_value},
    piece_components::VisualInfo,
    quadtree::QuadTree,
    renderer::{GlobalAssets, IRenderer},
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
            if value == Value::Nil {
                String::from("default")
            } else {
                match String::try_from_lua_value(&value) {
                    Ok(v) => v,
                    Err(msg) => return Err(format!("Invalid layer field: {msg}")),
                }
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

    /// texture
    pub texture: String,

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
            match String::try_from_lua_value(&val) {
                Ok(t) => t,
                Err(msg) => return Err(format!("Invalid texture field: {msg}")),
            }
        } else {
            return Err(format!("Invalid name field."));
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

    /// sets the categories of a piece
    pub fn set_categories(&mut self, piece: PieceEntity, categories: &[String]) {
        self.clear_categories(piece);
        self.add_categories(piece, categories);
    }

    /// clears all categories from a piece
    pub fn clear_categories(&mut self, piece: PieceEntity) {
        let piece_categories = if let Some(val) = self.piece_categories.remove(piece) {
            val
        } else {
            return;
        };

        for category_entity in piece_categories.iter() {
            let category_collection = self.category_collections.get_mut(category_entity).unwrap();
            category_collection.remove(piece);
            if category_collection.is_empty() {
                self.category_collections.remove(category_entity);
            }
        }
    }

    /// removes specific categories from a piece
    pub fn remove_categories(&mut self, piece: PieceEntity, categories: &[String]) {
        let piece_categories = if let Some(val) = self.piece_categories.get_mut(piece) {
            val
        } else {
            return;
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
        } else {
            // handle case where piece does not yet exist in this manager
            self.piece_draw_layers.insert(entity, layer);
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

    /// gets the name corresponding to the layer
    pub fn get_name(&self, layer: CollisionLayerEntity) -> Option<&String> {
        self.layers.get_name(layer)
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

    /// returns the number of collision layers contained
    pub fn len(&self) -> usize {
        self.layers.len()
    }
}

pub struct QuadTreeManager<R: IRect> {
    /// aabbs for each piece
    aabbs: DenseComponentManager<PieceEntity, R>,

    /// the quadtree used to track collisions
    quadtree: QuadTree<PieceEntity, R>,
}

impl<R: IRect> QuadTreeManager<R> {
    pub fn new<V: IVec2>(dimensions: V) -> Self {
        Self {
            aabbs: Default::default(),
            quadtree: QuadTree::new(R::from_position_dimension((0.0, 0.0), dimensions)),
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

    /// queries the quadtree for entity aabbs on the given point
    pub fn query_point<V: IVec2>(&self, point: V) -> Vec<PieceEntity> {
        self.quadtree
            .query_point(point, &|e| *self.aabbs.get(*e).unwrap())
    }

    /// queries the quadtree for entity aabbs on the given rectangle
    pub fn query_rect(&self, rect: R) -> Vec<PieceEntity> {
        self.quadtree
            .query_range(rect, &|e| *self.aabbs.get(*e).unwrap())
    }

    /// gets the rectangle the quadtree occupies
    pub fn rect(&self) -> R {
        self.quadtree.aabb()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlayerColor {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Lime,
    Cyan,
    Black,
    White,
    Brown,
}

impl PlayerColor {
    pub const COLOR_COUNT: usize = 12;

    pub fn into_color<C: IColor>(&self) -> C {
        let hex = match self {
            Self::Red => "#f16060ff",
            Self::Orange => "#f38e41ff",
            Self::Yellow => "#eec850ff",
            Self::Green => "#53b976ff",
            Self::Blue => "#63abecff",
            Self::Purple => "#bd6dd7ff",
            Self::Pink => "#fb9debff",
            Self::Lime => "#b8e36cff",
            Self::Cyan => "#7ee3e3ff",
            Self::Black => "#4b5060ff",
            Self::White => "#fff7eeff",
            Self::Brown => "#a86b55ff",
        };
        C::try_from_hex_str(hex).unwrap()
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let name_lower = name.to_ascii_lowercase();
        let name_lower_str: &str = &name_lower;
        Some(match name_lower_str {
            "red" => Self::Red,
            "orange" => Self::Orange,
            "yellow" => Self::Yellow,
            "green" => Self::Green,
            "blue" => Self::Blue,
            "purple" => Self::Purple,
            "pink" => Self::Pink,
            "lime" => Self::Lime,
            "cyan" => Self::Cyan,
            "black" => Self::Black,
            "white" => Self::White,
            "brown" => Self::Brown,
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
            Self::Pink => "Pink",
            Self::Lime => "Lime",
            Self::Cyan => "Cyan",
            Self::Black => "Black",
            Self::White => "White",
            Self::Brown => "Brown",
        }
    }
}

impl Display for PlayerColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl FromLuaValue for PlayerColor {
    fn try_from_lua_value(value: &Value) -> Result<Self, String> {
        match String::try_from_lua_value(value) {
            Ok(s) => match Self::from_name(&s) {
                Some(color) => Ok(color),
                None => Err(format!("Invalid player color '{s}'.")),
            },
            Err(msg) => Err(format!("Invalid player color: {msg}")),
        }
    }
}

impl EntityTrait for PlayerColor {
    fn id(&self) -> u32 {
        match self {
            Self::Red => 0,
            Self::Orange => 1,
            Self::Yellow => 2,
            Self::Green => 3,
            Self::Blue => 4,
            Self::Purple => 5,
            Self::Pink => 6,
            Self::Lime => 7,
            Self::Cyan => 8,
            Self::Black => 9,
            Self::White => 10,
            Self::Brown => 11,
        }
    }

    fn from_id(id: usize) -> Self {
        match id {
            0 => Self::Red,
            1 => Self::Orange,
            2 => Self::Yellow,
            3 => Self::Green,
            4 => Self::Blue,
            5 => Self::Purple,
            6 => Self::Pink,
            7 => Self::Lime,
            8 => Self::Cyan,
            9 => Self::Black,
            10 => Self::White,
            11 => Self::Brown,
            _ => panic!("PlayerColor cannot be construccted from ids over 11."),
        }
    }
}

#[derive(Debug)]
pub struct PlayerManager<V: IVec2> {
    /// player colors mapped to their screen names
    players: DenseComponentManager<PlayerColor, String>,

    /// screen names mapped to their player colors
    colors: [String; PlayerColor::COLOR_COUNT],

    /// positions of player's hands
    positions: DenseComponentManager<PlayerColor, V>,

    /// the piece being grabbed by players
    grab_targets: DenseComponentManager<PlayerColor, PieceEntity>,

    /// the player grabbing a given piece
    rev_grab_targets: DenseComponentManager<PieceEntity, PlayerColor>,
}

impl<V: IVec2> Default for PlayerManager<V> {
    fn default() -> Self {
        Self {
            players: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            colors: [const { String::new() }; PlayerColor::COLOR_COUNT],
            positions: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            grab_targets: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            rev_grab_targets: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
        }
    }
}

impl<V: IVec2> PlayerManager<V> {
    pub const NAME_CHAR_LIMIT: usize = 12;

    /// gets the regex for valid names
    /// has the following requirements:
    ///  - 1-12 characters
    ///  - only graphic characters
    pub fn is_name_valid(name: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err(format!("Player name must not be empty."));
        }

        let mut len = 0;
        let mut chars = name.chars();
        while len <= Self::NAME_CHAR_LIMIT {
            len += 1;
            if let Some(ch) = chars.next() {
                if !Self::is_name_char_valid(ch) {
                    return Err(format!(
                        "Player names must only contain graphic characters."
                    ));
                }
            } else {
                return Ok(());
            }
        }
        Err(format!("Player names must not exceed 12 characters."))
    }

    /// returns an iterator over all held pieces
    pub fn grab_target_iter(&self) -> impl Iterator<Item = PieceEntity> {
        self.grab_targets.component_iter().cloned()
    }

    /// returns an iterator over players and their positions
    pub fn position_iter(&self) -> impl Iterator<Item = (PlayerColor, V)> {
        self.positions.pairs_iter().map(|(p, t)| (p, *t))
    }

    /// checks if a character is valid for use in a name
    /// must be a graphic character
    pub fn is_name_char_valid(ch: char) -> bool {
        ch.is_ascii_graphic()
    }

    /// creates a new player with the given name and color
    pub fn new_player(&mut self, color: PlayerColor, name: String) -> Result<(), String> {
        Self::is_name_valid(&name)?;
        if self.players.contains(color) {
            return Err(format!("Player color {color} is already taken."));
        }
        if self.colors.contains(&name) {
            return Err(format!("Player name '{name}' is already taken."));
        }

        self.players.insert(color, name.clone());
        self.colors[color.id() as usize] = name;
        self.positions.insert(color, V::from_xy(0.0, 0.0));
        Ok(())
    }

    /// gets a player color by name
    pub fn color_of(&self, name: &String) -> Option<PlayerColor> {
        self.colors
            .iter()
            .position(|n| n == name)
            .map(|id| PlayerColor::from_id(id))
    }

    /// gets the name of a player by color
    pub fn name_of(&self, color: PlayerColor) -> Option<&String> {
        self.players.get(color)
    }

    /// checks if a player is contained
    pub fn is_playing(&self, color: PlayerColor) -> bool {
        self.players.contains(color)
    }

    /// removes a player, does nothing if the player does not exist
    pub fn remove_player(&mut self, player: PlayerColor) {
        self.colors[player.id() as usize] = String::new();
        self.players.remove(player);
        self.positions.remove(player);
    }

    /// gets the position of the given player
    pub fn get_position(&self, player: PlayerColor) -> Option<V> {
        self.positions.get(player).copied()
    }

    /// sets the position of the given player
    pub fn set_position(&mut self, player: PlayerColor, pos: V) {
        if let Some(old_pos) = self.positions.get_mut(player) {
            *old_pos = pos;
        }
    }

    /// checks if the player is grabbing a piece
    pub fn is_grabbing(&self, player: PlayerColor) -> bool {
        self.grab_targets.contains(player)
    }

    /// checks if a piece is being grabbed by any of the player
    pub fn is_grabbed(&self, piece: PieceEntity) -> bool {
        self.rev_grab_targets.contains(piece)
    }

    /// gets the player grabbing the given piece
    pub fn get_grabber(&self, piece: PieceEntity) -> Option<PlayerColor> {
        self.rev_grab_targets.get(piece).copied()
    }

    /// gets the piece that the player is grabbing
    pub fn get_grab_target(&self, player: PlayerColor) -> Option<PieceEntity> {
        self.grab_targets.get(player).copied()
    }

    /// sets the piece that the player is grabbing, returning the old grab target
    /// panics if the target is already being grabbed by another player
    pub fn set_grab_target(
        &mut self,
        player: PlayerColor,
        target: Option<PieceEntity>,
    ) -> Option<PieceEntity> {
        if let Some(t) = target {
            assert!(!self.rev_grab_targets.contains(t));
            self.grab_targets.insert(player, t)
        } else {
            if let Some(piece) = self.grab_targets.remove(player) {
                self.rev_grab_targets.remove(piece);
                Some(piece)
            } else {
                None
            }
        }
    }

    /// returns an iterator over player colors and their names
    pub fn iter(&self) -> impl Iterator<Item = (PlayerColor, &String)> {
        self.players.pairs_iter()
    }
}

pub struct BoardManager<R: IRect, V: IVec2, S: IConvexShape + FromLuaTable, T, F> {
    /// textures resources that can be used
    /// unnamed textures must be uniquely owned
    texture_resources: TextureManager<T, F>,

    /// player management
    players: PlayerManager<V>,

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
    /// each piece must have a texture
    piece_textures: DenseComponentManager<PieceEntity, TextureEntity>,

    /// pieces who should have their collision shapes drawn
    debug_pieces: DenseSet<PieceEntity>,

    /// pieces who should be hidden
    hidden_pieces: DenseSet<PieceEntity>,

    /// pieces who have collisions disabled
    no_collision_pieces: DenseSet<PieceEntity>,

    /// the order in which to draw entities
    /// all piece entities are found in this vector, even if they do not have a texture
    /// this is so the draw order of pieces is stable even after adding or removing their
    /// texture component
    draw_order: VecDeque<PieceEntity>,

    /// the transforms of each piece
    /// each entity must have a transform
    piece_transforms: DenseComponentManager<PieceEntity, Transform<V>>,

    /// the colliders for each piece
    /// we must ensure that each piece collider vector is non-emtpy
    piece_colliders: DenseComponentManager<PieceEntity, Vec<PieceCollider>>,

    /// quadtree manager that tracks piece aabb collisions
    /// also tracks the board's dimensions
    quadtree: QuadTreeManager<R>,
}

impl<R: IRect, V: IVec2, S: IConvexShape + FromLuaTable, T, F> BoardManager<R, V, S, T, F> {
    pub fn new(dimensions: V) -> Self {
        Self {
            texture_resources: Default::default(),
            players: Default::default(),
            collision_shape_resources: Default::default(),
            collision_layers: Default::default(),
            draw_layers: Default::default(),
            categories: Default::default(),
            piece_entity_manager: Default::default(),
            debug_pieces: Default::default(),
            hidden_pieces: Default::default(),
            no_collision_pieces: Default::default(),
            stacks: Default::default(),
            piece_textures: Default::default(),
            draw_order: Default::default(),
            piece_transforms: Default::default(),
            piece_colliders: Default::default(),
            quadtree: QuadTreeManager::new(dimensions),
        }
    }

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

        self.is_texture_valid(&ir.texture)?;

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
    pub fn is_texture_valid(&self, texture: &String) -> Result<(), String> {
        if !self.texture_resources.contains_texture_name(&texture) {
            Err(format!("No texture has name {texture}."))
        } else {
            Ok(())
        }
    }

    /// sets the texture of a piece without checking whether the or texture is valid
    pub fn set_texture(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        texture: &String,
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece.clone())?;
        self.is_texture_valid(texture)?;

        self.set_piece_texture_unchecked(piece, texture);
        Ok(())
    }

    /// sets the texture of a piece without checking whether the or texture is valid
    fn set_piece_texture_unchecked(&mut self, piece: PieceEntity, texture: &String) {
        // determine texture entity
        let new_texture = self.texture_resources.get_texture_entity(texture).unwrap();

        // modify texture
        self.piece_textures.insert(piece, new_texture);
    }

    /// sets the piece or stacks transform
    pub fn set_transform(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        transform: Transform<V>,
    ) -> Result<(), String> {
        let piece = self.is_entity_valid(piece)?;
        self.set_piece_transform_unchecked(piece, transform);
        Ok(())
    }

    /// sets the piece or stack's offset
    pub fn set_position(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        offset: V,
    ) -> Result<(), String> {
        let piece = self.is_entity_valid(piece)?;

        let angle = self.piece_transforms.get(piece).unwrap().angle;

        let transform = Transform { offset, angle };

        self.set_piece_transform_unchecked(piece, transform);
        Ok(())
    }

    /// sets the piece or stack's angle
    pub fn set_angle(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        angle: f32,
    ) -> Result<(), String> {
        let piece = self.is_entity_valid(piece)?;

        let offset = self.piece_transforms.get(piece).unwrap().offset;

        let transform = Transform { offset, angle };

        self.set_piece_transform_unchecked(piece, transform);
        Ok(())
    }

    /// sets the categories of a piece
    pub fn set_categories(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        categories: &[String],
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;

        self.categories.set_categories(piece, categories);

        Ok(())
    }

    /// adds categories to a piece
    pub fn add_categories(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        categories: &[String],
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;

        self.categories.add_categories(piece, categories);

        Ok(())
    }

    /// removes categories from a piece
    pub fn remove_categories(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        categories: &[String],
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;

        self.categories.remove_categories(piece, categories);
        Ok(())
    }

    /// sets the transform of a piece without checking whether the piece or transform is valid
    fn set_piece_transform_unchecked(&mut self, piece: PieceEntity, transform: Transform<V>) {
        if let Some(colliders) = self.piece_colliders.get(piece) {
            // if this piece has colliders...
            // ensure they are fully contained by the board
            let initial_aabb: R = geometry::compute_advanced_aabb(
                &transform,
                colliders
                    .iter()
                    .map(|c| self.collision_shape_resources.get(c.shape).unwrap()),
            )
            .unwrap();
            let final_aabb: R = self.quadtree.rect().fit(initial_aabb);
            let offset = final_aabb.min::<V>().sub(initial_aabb.min::<V>());
            let transform = Transform {
                offset: transform.offset.add(offset),
                angle: transform.angle,
            };

            self.piece_transforms.insert(piece, transform);

            // update quadtree only if player is not grabbed
            // (setting aabb may insert it)
            if !self.players.is_grabbed(piece) {
                self.quadtree.set_aabb(piece, final_aabb);
            }
        } else {
            // clamp transform so that its origin fits in the board
            let transform = Transform {
                offset: self.quadtree.rect().fit_point(transform.offset),
                angle: transform.angle,
            };

            self.piece_transforms.insert(piece, transform);
        }
    }

    /// sets the colliders of the pice
    pub fn set_piece_colliders(
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
        self.quadtree.remove(piece);

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
            // update quadtree if the piece is not grabbed
            if !self.players.is_grabbed(piece) {
                let aabb = geometry::compute_advanced_aabb(
                    self.piece_transforms.get(piece).unwrap(),
                    colliders
                        .iter()
                        .map(|c| self.collision_shape_resources.get(c.shape).unwrap()),
                )
                .unwrap();
                self.quadtree.set_aabb(piece, aabb);
            }

            // set colliders
            if let Some(colliders) = self.piece_colliders.insert(piece, colliders) {
                // decrement count of old colliders
                for collider in colliders {
                    self.collision_layers.decrement_count(collider.layer);
                }
            }
        }
    }

    pub fn create_piece(&mut self, ir: PieceIR<S, V>) -> Result<PieceEntity, String> {
        self.validate_piece_ir(&ir)?;

        let entity = if let Some(name) = &ir.name {
            self.piece_entity_manager
                .get_new_named(name.clone())
                .unwrap()
        } else {
            self.piece_entity_manager.get_new()
        };

        self.set_piece_texture_unchecked(entity, &ir.texture);

        self.draw_layers.set(entity, ir.draw_layer);

        self.categories.add_categories(entity, &ir.categories);

        self.draw_order.push_back(entity);

        self.set_piece_transform_unchecked(entity, ir.transform);

        self.set_piece_colliders_unchecked(entity, ir.colliders);

        Ok(entity)
    }

    /// returns an iterator over the existing draw layers in sorted order
    pub fn draw_layer_iter(&self) -> impl Iterator<Item = i32> {
        self.draw_layers.layer_iter()
    }

    /// returns an iterator over texture-owning pieces and their transforms in the order in which
    /// they should be drawn (appearing first means should be on the bottom)
    pub fn texture_draw_iter(
        &self,
    ) -> impl Iterator<Item = (&T, Option<&[PieceCollider]>, Transform<V>, i32)> {
        self.draw_order.iter().filter_map(|piece| {
            if self.hidden_pieces.contains(*piece) {
                return None;
            }

            // entity unwrap is safe because we guarantee all pieces have a texture
            let texture_entity = self.piece_textures.get(*piece).unwrap();

            // resource unwrap is safe because all piece_texture components are valid
            let texture = &self
                .texture_resources
                .get_texture(*texture_entity)
                .unwrap()
                .texture;
            let transform = self.piece_transforms.get(*piece).unwrap();
            let draw_layer = self.draw_layers.get(*piece).unwrap();
            let colliders = if self.debug_pieces.contains(*piece) {
                self.piece_colliders.get(*piece).map(|l| l.as_slice())
            } else {
                None
            };

            Some((texture, colliders, *transform, draw_layer))
        })
    }

    /// gets the collision shape referenced by the CollisionShapeEntity
    pub fn get_collision_shape(&self, shape: CollisionShapeEntity) -> Option<&CollisionShape<S>> {
        self.collision_shape_resources.get(shape)
    }

    /// gets the number of collision layers
    pub fn collision_layer_count(&self) -> usize {
        self.collision_layers.len()
    }

    /// returns an iterator over the textures of grabbed pieces and their transforms
    /// grab targets should be drawn on top of all other pieces
    pub fn grab_target_draw_iter(
        &self,
    ) -> impl Iterator<Item = (&T, Option<&[PieceCollider]>, Transform<V>)> {
        self.players.grab_target_iter().filter_map(|target| {
            if self.hidden_pieces.contains(target) {
                return None;
            }

            let texture_entity = self.piece_textures.get(target)?;
            let texture = &self
                .texture_resources
                .get_texture(*texture_entity)
                .unwrap()
                .texture;
            let transform = self.piece_transforms.get(target).unwrap();
            let colliders = if self.debug_pieces.contains(target) {
                self.piece_colliders.get(target).map(|l| l.as_slice())
            } else {
                None
            };

            Some((texture, colliders, *transform))
        })
    }

    /// returns an iterator over players, their positions, and their held pieces
    /// players should be drawn on top of every piece
    pub fn player_draw_iter<'a, 'b, C: IColor>(
        &'b self,
        global_assets: &'a GlobalAssets<T>,
    ) -> impl Iterator<Item = (&'a T, V, C)> {
        self.players.position_iter().map(|(color, pos)| {
            let texture = if self.players.is_grabbing(color) {
                &global_assets.closed_hand_texture
            } else {
                &global_assets.open_hand_texture
            };
            (texture, pos, color.into_color())
        })
    }

    /// gets the dimensions of the board
    pub fn dimensions(&self) -> V {
        self.quadtree.rect().dimensions()
    }

    /// attempts to create a new texture, returning its entity on success
    /// textures must be assigned a name
    pub fn create_texture<C, IR>(
        &mut self,
        project_path: &Path,
        name: String,
        visual: &VisualInfo<C, R, V>,
        magnification_dimensions: V,
        renderer: &mut IR,
    ) -> Result<(), String>
    where
        C: IColor,
        IR: IRenderer<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        self.texture_resources.load_texture(
            project_path,
            Some(name.clone()),
            visual,
            magnification_dimensions,
            renderer,
        )?;

        Ok(())
    }

    pub fn add_player(&mut self, color: PlayerColor, name: String) -> Result<(), String> {
        self.players.new_player(color, name)
    }

    pub fn remove_player(&mut self, color: PlayerColor) {
        self.players.remove_player(color);
    }

    /// gets the name of the texture that a piece contains
    pub fn get_texture(
        &self,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<Option<&String>, String> {
        let piece = self.is_piece_valid(piece)?;

        Ok(self
            .piece_textures
            .get(piece)
            .map(|i| self.texture_resources.get_texture_name(*i).unwrap()))
    }

    /// gets the texture by name
    pub fn get_texture_resource(&self, name: &String) -> Option<&T> {
        self.texture_resources
            .get_texture_by_name(name)
            .map(|t| &t.texture)
    }

    /// returns a lua table based on the pieces intersecting the given piece
    ///
    /// the output table has both sequential and non-sequential values
    /// sequential values are the names of the overlapping collision layers
    /// non-sequential values are a map from the names of the overlapping collision layers
    /// to the pieces overlapping in that layer
    pub fn query_piece_overlaps(&self, lua: &Lua, piece: PieceEntity) -> mlua::Table {
        if self.no_collision_pieces.contains(piece) {
            return lua.create_table().expect("unable to create empty table");
        }

        let colliders = if let Some(c) = self.piece_colliders.get(piece) {
            c
        } else {
            return lua.create_table().expect("unable to create empty table");
        };

        // group colliders by layer
        let shapes_by_layer = {
            let mut out = DenseComponentManager::new();

            for collider in colliders.iter() {
                let shape_list = out.get_mut_or_insert(collider.layer, || Vec::new());
                let shape = self.collision_shape_resources.get(collider.shape).unwrap();
                shape_list.push(shape);
            }

            out
        };

        // can unwrap transform because all pieces have transforms
        let transform = self.piece_transforms.get(piece).unwrap();

        // can unwrap aabb because only non-empty collider listsare inserted into collider component
        let aabb = geometry::compute_advanced_aabb(
            transform,
            colliders
                .iter()
                .map(|c| self.collision_shape_resources.get(c.shape).unwrap()),
        )
        .unwrap();

        let mut out = DenseComponentManager::new();

        'aabb_loop: for p in self.quadtree.query_rect(aabb) {
            if p == piece || self.no_collision_pieces.contains(p) {
                continue 'aabb_loop;
            }

            // unwrap is safe because pieces in quadtree must have colliders
            let p_colliders = self.piece_colliders.get(p).unwrap();

            // can unwrap transform because all pieces have transforms
            let p_transform = self.piece_transforms.get(p).unwrap();

            // the set of layers in this piece that are already contained
            let mut overlapping_layers = DenseSet::new();

            'collider_loop: for p_collider in p_colliders {
                // skip layers already checked
                if overlapping_layers.contains(p_collider.layer) {
                    continue 'collider_loop;
                }

                // skip layers that the piece does not have
                let shapes = if let Some(s) = shapes_by_layer.get(p_collider.layer) {
                    s
                } else {
                    continue 'collider_loop;
                };

                // unwrap is safe because all collider ids are contained in collision shape resources
                let p_shape = self
                    .collision_shape_resources
                    .get(p_collider.shape)
                    .unwrap();

                'shape_loop: for shape in shapes {
                    if shape.intersects(transform, p_shape, p_transform) {
                        let pieces_in_layer =
                            out.get_mut_or_insert(p_collider.layer, || Vec::new());
                        pieces_in_layer.push(p);
                        overlapping_layers.insert(p_collider.layer);

                        break 'shape_loop;
                    }
                }
            }
        }

        let table = lua
            .create_table_with_capacity(out.len(), out.len())
            .expect("Unable to create piece overlap query table");
        let mut layer_index = 0;
        for (layer, pieces) in out.pairs_iter() {
            layer_index += 1;

            let layer_table = lua
                .create_table_with_capacity(pieces.len(), 0)
                .expect("Unable to create piece overlap query table");

            let mut piece_index = 0;
            for piece in pieces {
                piece_index += 1;

                layer_table
                    .raw_set(piece_index, piece.id())
                    .expect("Unable to create piece overlap query table");
            }

            let layer_name = lua
                .create_string(&self.collision_layers.get_name(layer).unwrap())
                .expect("Unable to create piece overlap query table");

            table
                .raw_set(&layer_name, layer_table)
                .expect("Unable to create piece overlap query table");
            table
                .raw_set(layer_index, layer_name)
                .expect("Unable to create piece overlap query table");
        }

        table
    }

    /// checks if a texture with the given name exists
    pub fn contains_texture(&self, name: &String) -> bool {
        self.texture_resources.contains_texture_name(name)
    }

    /// makes a player grab or release a piece, returns true on successful operation
    /// the operation may only fail for invalid players or if a piece is being picked up that is already
    pub fn set_grab(&mut self, player: PlayerColor, piece: Option<PieceEntity>) -> bool {
        if !self.players.is_playing(player) {
            return false;
        }

        if let Some(piece) = piece {
            if !self.piece_entity_manager.is_used(piece) {
                return false;
            }
            if let Some(_) = self.players.get_grabber(piece) {
                return false;
            }

            self.grab_piece_unchecked(player, piece);
            true
        } else {
            self.drop_piece_unchecked(player);
            true
        }
    }

    /// returns the piece that a player is grabbing
    pub fn grabbed_piece(&self, player: PlayerColor) -> Option<PieceEntity> {
        self.players.get_grab_target(player)
    }

    /// returns the piece a player should grab, should it attempt to
    pub fn piece_to_grab(&mut self, player: PlayerColor) -> Option<PieceEntity> {
        let position = self.players.get_position(player)?;

        // get pieces with overlapping aabbs
        let colliding_aabbs = self.quadtree.query_point(position);
        if colliding_aabbs.is_empty() {
            return None;
        }

        // filter out pieces that are invisible or aren't actually colliding
        let colliding = colliding_aabbs.into_iter().filter(|piece| {
            if self.hidden_pieces.contains(*piece) {
                return false;
            }

            let transform = *self.piece_transforms.get(*piece).unwrap();
            let shape_iter = self
                .piece_colliders
                .get(*piece)
                .unwrap()
                .iter()
                .map(|collider| self.collision_shape_resources.get(collider.shape).unwrap());

            for shape in shape_iter {
                if shape.contains_point(transform, position) {
                    return true;
                }
            }
            false
        });

        // find the highest draw layer
        // also if colliding is empty, return
        let draw_layer = colliding
            .clone()
            .map(|piece| self.draw_layers.get(piece).unwrap())
            .max()?;

        // place all pieces in the highest draw layer into a set
        let mut high_colliding = {
            let high_colliding_iter =
                colliding.filter(|piece| self.draw_layers.get(*piece).unwrap() == draw_layer);
            let mut set = DenseSet::new();
            for piece in high_colliding_iter {
                set.insert(piece);
            }
            set
        };

        // get and return the highest piece
        for piece in self.draw_order.iter().copied() {
            high_colliding.remove(piece);

            if high_colliding.is_empty() {
                return Some(piece);
            }
        }

        unreachable!()
    }

    /// makes a piece show its collision shapes
    pub fn show_debug(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.debug_pieces.insert(piece);
        Ok(())
    }

    /// makes a piece hide its collision shapes
    pub fn hide_debug(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.debug_pieces.remove(piece);
        Ok(())
    }

    /// makes a player grab the given piece
    /// fails without error if the piece is being held or the player is already holding a piece
    /// returns true on successful grab without error
    pub fn attempt_grab(
        &mut self,
        player: PlayerColor,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<bool, String> {
        let piece = self.is_piece_valid(piece)?;
        if !self.players.is_playing(player) {
            return Err(format!("Player {player} does not exist."));
        }

        if self.players.is_grabbing(player) {
            return Ok(false);
        }
        if self.players.is_grabbed(piece) {
            return Ok(false);
        }
        self.grab_piece_unchecked(player, piece);
        Ok(true)
    }

    /// makes a player grab a given piece
    /// no checks are performed to ensure that the piece or player is valid and not already grabbed
    fn grab_piece_unchecked(&mut self, player: PlayerColor, piece: PieceEntity) {
        // disable collisions
        self.quadtree.remove(piece);

        // actually grabbing the piece
        self.players.set_grab_target(player, Some(piece));

        // move player inbounds
        self.set_player_position(player, self.players.get_position(player).unwrap());
    }

    /// makes the player drop is held piece
    pub fn drop_held_piece(&mut self, player: PlayerColor) {
        if self.players.is_grabbing(player) {
            self.drop_piece_unchecked(player);
        }
    }

    /// makes a player drop the piece it is grabbing
    /// no checks are performed to ensure that the player is valid
    /// puts the newly dropped piece at the top of its draw layer
    fn drop_piece_unchecked(&mut self, player: PlayerColor) {
        // drop the piece
        let piece = self.players.set_grab_target(player, None).unwrap();

        // re-enable collisions by performing an operation that updates the quadtree
        self.set_piece_transform_unchecked(piece, *self.piece_transforms.get(piece).unwrap());

        // put new piece at top of its draw layer
        self.draw_order
            .remove(self.draw_order.iter().position(|p| *p == piece).unwrap());
        self.draw_order.push_back(piece);
    }

    /// sets the position of the player
    pub fn set_player_position(&mut self, player: PlayerColor, position: V) {
        if !self.players.is_playing(player) {
            return;
        }

        // clamp position to board boundaries
        let position = self.quadtree.rect().fit_point(position);

        if let Some(piece) = self.players.get_grab_target(player) {
            // update grabbed piece position
            // use updated position to set player position (preserves clamping rules)
            let _ = self.set_position(EntityIdentifier::Entity(piece), position);
            self.players
                .set_position(player, self.piece_transforms.get(piece).unwrap().offset);
        } else {
            // update player position
            self.players.set_position(player, position);
        }
    }

    /// makes a piece visible
    pub fn show(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.hidden_pieces.remove(piece);
        Ok(())
    }

    /// makes a piece hidden
    pub fn hide(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.hidden_pieces.insert(piece);
        Ok(())
    }

    /// disable a piece's collisions
    pub fn disable_collisions(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.no_collision_pieces.insert(piece);
        Ok(())
    }

    /// enable a piece's collisions
    pub fn enable_collisions(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.no_collision_pieces.remove(piece);
        Ok(())
    }
}
