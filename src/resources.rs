use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fmt::Display,
    iter::Cloned,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use mlua::Value;
use roaring::RoaringBitmap;

use crate::{
    color::IColor,
    ecs::{DenseComponentManager, DenseSet, Entity, EntityTrait, ResourceManager},
    geometry::{IRect, IVec2},
    intermediate_representations::VisualInfo,
    lua::FromLuaValue,
    quadtree::QuadTree,
    renderer::IRenderer,
};

#[cfg(target_os = "windows")]
/// gets all possible font paths provided in order the that they should be searched, including:
///  - Project-local fonts
///  - User fonts
///  - System fonts
pub fn get_font_paths(project_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    paths.push(project_path.join("fonts"));
    if let Some(home) = std::env::home_dir() {
        paths.push(home.join(r"AppData\Local\Microsoft\Windows\Fonts"))
    }
    paths.push(PathBuf::from(r"C:\Windows\Fonts"));

    paths
}

#[cfg(target_os = "linux")]
/// gets all possible font paths provided in order the that they should be searched, including:
///  - System fonts
///  - User fonts
///  - Project-local fonts
pub fn font_paths(&self) -> PathBuf {
    todo!()
}

#[cfg(target_os = "macos")]
/// gets all possible font paths provided in order the that they should be searched, including:
///  - System fonts
///  - User fonts
///  - Project-local fonts
pub fn font_paths(&self) -> PathBuf {
    todo!()
}

pub fn get_image_path(project_path: &Path) -> PathBuf {
    project_path.join("images")
}

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
pub struct PileEntry<V: IVec2> {
    /// the pieces on this pile from deepest to topmost
    pub pieces: VecDeque<PieceEntity>,

    /// the offset of the snapping circle
    pub snap_offset: V,

    /// the radius of the snapping cirle
    /// circle should be centered on the top piece
    pub snap_radius: f32,
}

/// manages piles and the pieces on them
pub struct PileManager<V: IVec2> {
    piles: DenseComponentManager<PieceEntity, PileEntry<V>>,
    piece_pile_map: DenseComponentManager<PieceEntity, PieceEntity>,
}

impl<V: IVec2> Default for PileManager<V> {
    fn default() -> Self {
        Self {
            piles: Default::default(),
            piece_pile_map: Default::default(),
        }
    }
}

impl<V: IVec2> PileManager<V> {
    /// check if an entity is a pile
    pub fn is_pile(&self, pile: PieceEntity) -> bool {
        self.piles.contains(pile)
    }

    /// checks if an entity is on a pile
    pub fn is_on_pile(&self, piece: PieceEntity) -> bool {
        self.piece_pile_map.contains(piece)
    }

    /// gets the pile that the piece is on, if it exists
    /// note that if the given entity is a pile, None is returned
    pub fn get_containing_pile(&self, piece: PieceEntity) -> Option<PieceEntity> {
        self.piece_pile_map.get(piece).cloned()
    }

    /// gets the snap offset of the pile
    pub fn get_snap_offset(&self, pile: PieceEntity) -> Option<V> {
        self.piles.get(pile).map(|s| s.snap_offset)
    }

    /// gets the pieces on the given pile
    pub fn get_pieces_on_pile(
        &self,
        pile: PieceEntity,
    ) -> Cloned<std::collections::vec_deque::Iter<'_, PieceEntity>> {
        if let Some(entry) = self.piles.get(pile) {
            entry.pieces.iter().cloned()
        } else {
            std::collections::vec_deque::Iter::default().cloned()
        }
    }

    /// gets the number of pile contained
    pub fn pile_count(&self) -> usize {
        self.piles.len()
    }

    /// gets the number of pieces on piles
    pub fn piled_count(&self) -> usize {
        self.piece_pile_map.len()
    }

    /// creates a new pile
    /// panics if the given entity is already a pile or another piece
    pub fn create_pile(&mut self, entity: PieceEntity, snap_radius: f32, snap_offset: V) {
        debug_assert!(
            self.is_pile(entity),
            "Attempted to recreate an existing pile."
        );
        debug_assert!(
            self.is_on_pile(entity),
            "Attempted to convert a piece to a pile."
        );

        self.piles.insert(
            entity,
            PileEntry {
                pieces: VecDeque::new(),
                snap_offset,
                snap_radius,
            },
        );
    }

    /// removes an item from any pile it may be a part of
    pub fn unpile(&mut self, piece: PieceEntity) {
        if let Some(pile) = self.get_containing_pile(piece) {
            let pieces = &mut self.piles.get_mut(pile).unwrap().pieces;
            pieces.remove(pieces.iter().position(|p| *p == piece).unwrap());
        }
    }

    /// appends the contents of the the top pile onto the bottom pile
    /// panics if the piece pile being pushed is non-existent
    pub fn push_pile(&mut self, bottom_pile: PieceEntity, top_pile: PieceEntity) {
        debug_assert!(
            !self.is_pile(bottom_pile),
            "Attempted to push to a non-existent pile."
        );
        debug_assert!(
            !self.is_pile(top_pile),
            "Attempted to push a non-existent pile."
        );

        let top_data = self.piles.remove(top_pile).unwrap();
        let bottom_data = self.piles.get_mut(bottom_pile).unwrap();
        for piece in top_data.pieces {
            self.piece_pile_map.insert(piece, bottom_pile);
            bottom_data.pieces.push_back(piece);
        }
    }

    /// destroys a pile
    /// returns a vector containing all contained pieces in order from bottom to top
    /// will return an empty vector if the pile does not exist or is empty
    pub fn remove_pile(&mut self, pile: PieceEntity) -> VecDeque<PieceEntity> {
        if let Some(data) = self.piles.remove(pile) {
            for piece in data.pieces.iter() {
                self.piece_pile_map.remove(*piece);
            }
            data.pieces
        } else {
            VecDeque::new()
        }
    }

    /// adds a piece to the top of the given pile
    /// panics if the piece being pushed already on a pile or if the pile doesnt exist
    pub fn push_top(&mut self, pile: PieceEntity, piece: PieceEntity) {
        debug_assert!(
            !self.is_on_pile(piece),
            "You may not push an already-piled piece onto different pile."
        );
        debug_assert!(
            !self.is_pile(pile),
            "Attempted to push to a non-existent pile."
        );

        self.piles.get_mut(pile).unwrap().pieces.push_back(piece);
    }

    /// adds a piece to the bottom of the given pile
    /// panics if the piece being pushed already on a pile or if the pile doesnt exist
    pub fn push_bottom(&mut self, pile: PieceEntity, piece: PieceEntity) {
        debug_assert!(
            !self.is_on_pile(piece),
            "You may not push an already-piled piece onto different pile."
        );
        debug_assert!(
            !self.is_pile(pile),
            "Attempted to push to a non-existent pile."
        );

        self.piles.get_mut(pile).unwrap().pieces.push_front(piece);
    }

    /// pops the bottom piece from a pile
    pub fn pop_bottom(&mut self, pile: PieceEntity) -> Option<PieceEntity> {
        let piece = self.piles.get_mut(pile)?.pieces.pop_front()?;
        self.piece_pile_map.remove(piece);
        Some(piece)
    }

    /// pops the top piece from a pile
    pub fn pop_top(&mut self, pile: PieceEntity) -> Option<PieceEntity> {
        let piece = self.piles.get_mut(pile)?.pieces.pop_back()?;
        self.piece_pile_map.remove(piece);
        Some(piece)
    }
}

pub struct CategoryManager {
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

pub struct DrawLayerManager {
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

pub struct CollisionLayerManager {
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

/// texture info for a piece face
#[derive(Debug, Clone)]
pub struct TextureInfo<T> {
    /// the texture used to draw this piece
    pub texture: T,

    /// an enlarged version of this texture for zoomed views
    pub texture_magnified: T,
}

pub struct VisualComponentProvider<'a, F, I> {
    /// fonts usable in rendering visuals
    fonts: *mut FontManager<F>,

    /// the images used for rendering visuals
    images: *mut HashMap<String, I>,

    /// the project's directory
    project_path: &'a Path,

    marker: PhantomData<&'a ()>,
}

impl<'a, F, I> VisualComponentProvider<'a, F, I> {
    pub fn new(
        project_path: &'a Path,
        fonts: &'a mut FontManager<F>,
        images: &'a mut HashMap<String, I>,
    ) -> Self {
        Self {
            project_path,
            fonts: fonts as *mut FontManager<F>,
            images: images as *mut HashMap<String, I>,
            marker: PhantomData,
        }
    }

    pub fn get_or_load_font<R: IRenderer<Font = F>>(
        &mut self,
        renderer: &mut R,
        font_name: &String,
        size: u32,
        characters: &str,
    ) -> Option<&F> {
        // SAFETY: only one font reference can be acquired at the same time because this struct is
        // borrowed mutably, there is no danger of multiple mutable references to the font manager
        let fonts = unsafe { &mut *self.fonts };
        if let Some(f) = fonts.get_font(font_name, size, characters) {
            return Some(f);
        }

        let fonts = unsafe { &mut *self.fonts };
        fonts.load_font(renderer, self.project_path, font_name, size, characters);
        fonts.get_font(font_name, size, characters)
    }

    pub fn get_or_load_image<R: IRenderer<Texture = I>>(
        &mut self,
        renderer: &mut R,
        path: &String,
    ) -> Option<&I> {
        // SAFETY: only one image reference can be acquired at the same time because this struct is
        // borrowed mutably, there is no danger of multiple mutable references to the image manager
        let images = unsafe { &mut *self.images };
        if let Some(i) = images.get(path) {
            return Some(i);
        }

        let images = unsafe { &mut *self.images };
        let path_buf = get_image_path(self.project_path).join(path);
        let image = if let Ok(i) = renderer.load_texture(&path_buf) {
            i
        } else {
            return None;
        };
        images.insert(path.clone(), image);
        images.get(path)
    }
}

/// an entry in a font manager for fonts saved to memory
pub struct FontEntry<F> {
    /// the set of characters loaded for this font
    pub charset: RoaringBitmap,

    /// a map from font size to font
    pub sizes: BTreeMap<u32, F>,
}

pub struct FontManager<F> {
    fonts: HashMap<String, FontEntry<F>>,
}

impl<F> Default for FontManager<F> {
    fn default() -> Self {
        Self {
            fonts: Default::default(),
        }
    }
}

impl<F> FontManager<F> {
    pub fn new() -> Self {
        Self::default()
    }

    /// batch loads multiple font sizes at once
    /// returns true if all sizes could be loaded
    pub fn batch_load_font<R: IRenderer<Font = F>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        font_name: &String,
        sizes: &[u32],
        characters: &str,
    ) -> bool {
        for font_dir in get_font_paths(project_path) {
            if self.batch_load_font_helper(renderer, &font_dir, font_name, sizes, characters) {
                return true;
            }
        }

        false
    }

    /// batch loads multiple font sizes at once
    /// returns true if all sizes could be loaded
    pub fn batch_load_font_helper<R: IRenderer<Font = F>>(
        &mut self,
        renderer: &mut R,
        font_dir: &Path,
        font_name: &String,
        sizes: &[u32],
        characters: &str,
    ) -> bool {
        let path_buf = font_dir.join(font_name);

        if let Some(entry) = self.fonts.get_mut(font_name) {
            // update existing entry with new characters
            let mut needs_update = false;
            for c in characters.chars() {
                if entry.charset.insert(c as u32) {
                    needs_update = true;
                }
            }

            let characters: String = entry
                .charset
                .iter()
                .map(|e| unsafe { char::from_u32_unchecked(e) })
                .collect();
            if needs_update {
                // reload all contained font sizes
                for (size, font) in entry.sizes.iter_mut() {
                    let font_result = renderer.load_font(&path_buf, *size, &characters);
                    if font_result.is_err() {
                        return false;
                    }
                    *font = font_result.unwrap();
                }
            }

            // insert the new font size
            for size in sizes {
                if entry.sizes.contains_key(size) {
                    continue;
                }

                let font_result = renderer.load_font(&path_buf, *size, &characters);
                if font_result.is_err() {
                    return false;
                }
                let font = font_result.unwrap();
                entry.sizes.insert(*size, font);
            }

            return true;
        }

        // completely new font entry
        let mut temp_map = BTreeMap::new();
        for size in sizes {
            let font_result = renderer.load_font(&path_buf, *size, characters);
            if font_result.is_err() {
                if !temp_map.is_empty() {
                    let entry = FontEntry {
                        charset: RoaringBitmap::from_iter(characters.chars().map(|c| c as u32)),
                        sizes: temp_map,
                    };
                    self.fonts.insert(font_name.clone(), entry);
                }

                return false;
            }
            temp_map.insert(*size, font_result.unwrap());
        }

        let entry = FontEntry {
            charset: RoaringBitmap::from_iter(characters.chars().map(|c| c as u32)),
            sizes: temp_map,
        };
        self.fonts.insert(font_name.clone(), entry);

        true
    }

    /// loads a single font, returning true on success or if the font was already loaded
    pub fn load_font<R: IRenderer<Font = F>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        font_name: &String,
        size: u32,
        characters: &str,
    ) -> bool {
        let sizes = [size; 1];
        self.batch_load_font(renderer, project_path, font_name, &sizes, characters)
    }

    pub fn get_font(&self, font_name: &String, size: u32, characters: &str) -> Option<&F> {
        let font_entry = self.fonts.get(font_name)?;
        if characters
            .chars()
            .position(|c| !font_entry.charset.contains(c as u32))
            .is_some()
        {
            None
        } else {
            font_entry.sizes.get(&size)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureEntityMarker;
pub type TextureEntity = Entity<TextureEntityMarker>;

/// texture resources
pub struct TextureManager<T, F> {
    /// loaded fonts to speed up font rendering capabilities
    fonts: FontManager<F>,

    /// loaded images to speed up image texture generation capabilities
    images: HashMap<String, T>,

    /// textures
    textures: ResourceManager<TextureEntity, TextureInfo<T>>,
}

impl<T, F> Default for TextureManager<T, F> {
    fn default() -> Self {
        Self {
            fonts: Default::default(),
            images: Default::default(),
            textures: Default::default(),
        }
    }
}

impl<T, F> TextureManager<T, F> {
    /// attempts to create a new texture, returning its entity on success
    /// textures may be assigned an optional name that can be used to identify it
    pub fn load_texture<C, R, V, IR>(
        &mut self,
        project_path: &Path,
        name: Option<String>,
        visual: &VisualInfo<C, R, V>,
        magnification_dimensions: V,
        renderer: &mut IR,
    ) -> Result<TextureEntity, String>
    where
        C: IColor,
        R: IRect,
        V: IVec2,
        IR: IRenderer<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        let provider = VisualComponentProvider {
            project_path,
            fonts: &mut self.fonts,
            images: &mut self.images,
            marker: PhantomData,
        };

        let texture = renderer.compile_visual(visual, magnification_dimensions, provider)?;

        if let Some(n) = name {
            if let Ok(entity) = self.textures.add_named(n.clone(), texture) {
                Ok(entity)
            } else {
                Err(format!("Texture with name '{n}' already exists."))
            }
        } else {
            Ok(self.textures.add(texture))
        }
    }

    /// removes a texture if it exists, returning it and its name
    pub fn remove_texture(
        &mut self,
        entity: TextureEntity,
    ) -> Option<(Option<String>, TextureInfo<T>)> {
        self.textures.remove(entity)
    }

    /// removes a texture by its name if it exists
    pub fn remove_texture_by_name(&mut self, name: &String) -> Option<TextureInfo<T>> {
        self.textures.remove_by_name(name)
    }

    /// checks if a texture with the given entity exists
    pub fn contains_texture(&self, entity: TextureEntity) -> bool {
        self.textures.contains(entity)
    }

    /// checks if a texture with the given name exists
    pub fn contains_texture_name(&self, name: &String) -> bool {
        self.textures.contains_name(name)
    }

    /// gets a texture by its entity
    pub fn get_texture(&self, entity: TextureEntity) -> Option<&TextureInfo<T>> {
        self.textures.get(entity)
    }

    /// gets a mutable texture by its entity
    pub fn get_texture_mut(&mut self, entity: TextureEntity) -> Option<&mut TextureInfo<T>> {
        self.textures.get_mut(entity)
    }

    /// gets a reference to the texture by name
    pub fn get_texture_by_name(&self, name: &String) -> Option<&TextureInfo<T>> {
        self.textures.get_by_name(name)
    }

    /// gets a mutable reference to the texture by name
    pub fn get_texture_mut_by_name(&mut self, name: &String) -> Option<&mut TextureInfo<T>> {
        self.textures.get_mut_by_name(name)
    }

    /// gets the name of the entity
    pub fn get_texture_name(&self, entity: TextureEntity) -> Option<&String> {
        self.textures.get_name(entity)
    }

    /// gets the entity by the given name
    pub fn get_texture_entity(&self, name: &String) -> Option<TextureEntity> {
        self.textures.get_entity(name)
    }

    /// returns an iterator over all entities, names, and textures
    pub fn iter(&self) -> impl Iterator<Item = (TextureEntity, Option<&String>, &TextureInfo<T>)> {
        self.textures.iter()
    }

    /// returns an iterator over entities and their names
    pub fn identifier_iter(&self) -> impl Iterator<Item = (TextureEntity, Option<&String>)> {
        self.textures.identifier_iter()
    }

    /// returns an iterator over all textures
    pub fn texture_iter(&self) -> impl Iterator<Item = &TextureInfo<T>> {
        self.textures.resource_iter()
    }

    /// returns a mutable iterator over all textures
    pub fn texture_iter_mut(&mut self) -> impl Iterator<Item = &mut TextureInfo<T>> {
        self.textures.resource_iter_mut()
    }

    /// loads a font, returning true on successful load
    pub fn load_font<R: IRenderer<Font = F>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        path: &String,
        sizes: &[u32],
        characters: &str,
    ) -> bool {
        self.fonts
            .batch_load_font(renderer, project_path, path, sizes, characters)
    }

    /// checks if a font with the given pathis loaded
    pub fn is_font_loaded(&self, path: &String, size: u32, characters: &str) -> bool {
        self.fonts.get_font(path, size, characters).is_some()
    }

    /// clears the cached fonts
    pub fn clear_font_cache(&mut self) {
        self.fonts = Default::default();
    }

    /// loads an image, returning true on successful load
    pub fn load_image<R: IRenderer<Texture = T>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        path: &String,
    ) -> bool {
        let path_buf = project_path.join(path);
        let image = if let Ok(i) = renderer.load_texture(&path_buf) {
            i
        } else {
            return false;
        };

        self.images.insert(path.clone(), image);
        true
    }

    /// checks if an image is loaded
    pub fn is_image_loaded(&self, path: &String) -> bool {
        self.images.contains_key(path)
    }

    /// clears the cached images
    pub fn clear_image_cache(&mut self) {
        self.images = Default::default();
    }
}
