use std::{
    collections::VecDeque,
    fmt::Debug,
    path::{Path, PathBuf},
};

use mlua::Lua;

use crate::{
    app::GlobalAssets,
    game::GameEntry,
    input::{InputEvent, PlayerInputEvent},
    intermediate_representations::{
        CollisionShapeIR, EntityIdentifier, ImageInfo, ImagePosition, PatternInfo, PatternType,
        PieceColliderIR, PieceIR, PileIR, TextboxInfo, VisualComponent, VisualInfo,
    },
    lua::FromLuaTable,
    platform::ITextureCompiler,
    resources::{
        CollisionLayerEntity, CollisionLayerManager, CollisionShapeEntity, DrawLayerManager,
        PieceCollider, PieceEntity, PileManager, PlayerColor, PlayerManager, QuadTreeManager,
        SnapTargetManager, TagManager, TextureEntity, TextureManager,
    },
    utils::color::IColor,
    utils::ecs::{
        DenseComponentManager, DenseSet, EntityTrait, NamedEntityManager, ResourceManager,
        SparseSet,
    },
    utils::geometry::{
        self, CollisionShape, IConvexShape, IRect, IVec2, NumericF32, RigidTransform,
    },
};

/// Manager for pieces on a board
/// Each PieceEntity falls under one of three mutually exclusive sets
///  - piles
///  - unpiled
///  - piled
#[derive(Debug)]
pub struct BoardManager<
    C: IColor,
    R: IRect,
    V: IVec2,
    S: IConvexShape + FromLuaTable + Debug + From<R>,
    T,
    F,
> {
    /// textures resources that can be used
    /// unnamed textures must be uniquely owned
    texture_manager: TextureManager<T, F>,

    /// player management
    player_manager: PlayerManager<V>,

    /// collision shape resources that can be referenced by pieces
    /// unnamed collision shapes must be uniquely owned
    collision_shape_manager: ResourceManager<CollisionShapeEntity, CollisionShape<S>>,

    /// a list of existing collision layers
    /// we will ensure that collision layers always have names
    collision_layer_manager: CollisionLayerManager,

    /// the tags of entities
    tag_manager: TagManager,

    /// the entity manager for pieces and piles
    piece_entity_manager: NamedEntityManager<PieceEntity>,

    /// piles of pieces
    pile_manager: PileManager<V>,

    /// manages the piles players snap to
    /// RULES:
    ///  - CONTAINED PLAYERS MUST be holding an item
    snap_manager: SnapTargetManager,

    /// the draw layers of pieces and piles
    /// RULES:
    ///  - ALL ENTITIES must have a draw layer
    entity_draw_layers: DrawLayerManager,

    /// the textures for each piece
    /// RULES:
    ///  - ALL PIECES must have a texture
    piece_textures: DenseComponentManager<PieceEntity, TextureEntity>,

    /// pieces who should have their collision shapes drawn
    /// RULES:
    ///  - NO PILES may be contained
    debug_entities: DenseSet<PieceEntity>,

    /// entities who should be hidden
    hidden_entities: DenseSet<PieceEntity>,

    /// pieces who have collisions disabled
    no_collision_entities: DenseSet<PieceEntity>,

    /// the order in which to draw entities
    /// RULES:
    ///  - NO PILED PIECES may be stored here
    entity_draw_order: VecDeque<PieceEntity>,

    /// the transforms of each entity
    /// piles and un-piled pieces use absolute transforms
    /// piled pieces have transforms relative to their parent pile
    /// RULES:
    ///  - ALL ENTITIES must have a transform
    entity_transforms: DenseComponentManager<PieceEntity, RigidTransform<V>>,

    /// the colliders for each piece
    /// we must ensure that each piece collider vector is non-emtpy
    /// RULES:
    ///  - NO PILES may have colliders
    piece_colliders: DenseComponentManager<PieceEntity, Vec<PieceCollider>>,

    /// quadtree manager that tracks piece aabb collisions
    /// also tracks the board's dimensions
    /// RULES:
    ///  - NO PILES may be contained
    ///  - HELD ENTITIES may NOT be contained
    ///  - HELD ITEMS may NOT be contained
    quadtree: QuadTreeManager<R>,

    /// quadtree that tracks snap positions
    /// RULES
    ///  - ONLY PILES may be contianed
    ///  - HELD PILES may NOT be contained
    snap_quadtree: QuadTreeManager<R>,

    /// dimensions of the visible area
    visible_rect: R,

    /// the background color of the board
    pub color: C,

    /// the rectangle that players can move freely in during lobby_mode
    /// relevant in lobby mode only
    lobby_rect: R,

    /// maximum scroll position for game selector mode
    /// relevant in selector mode only
    max_selector_scroll: f32,
}

impl<C: IColor, R: IRect, V: IVec2, S: IConvexShape + FromLuaTable + Debug + From<R>, T, F>
    BoardManager<C, R, V, S, T, F>
{
    /// the dimensions of quadtrees relative to actual dimensions
    const QUADTREE_SIZE_RATIO: f32 = 2.0;

    /// the max dimensions of magnified textures as a ratio of board size
    const MAGNIFICATION_RATIO: f32 = 0.8;

    pub fn new(dimensions: V) -> Self {
        let visible_rect = R::from_position_dimension((0.0, 0.0), dimensions);
        let quadtree_rect = R::from_center_dimension(
            visible_rect.center::<V>(),
            dimensions.scaled(Self::QUADTREE_SIZE_RATIO),
        );
        Self {
            texture_manager: Default::default(),
            player_manager: Default::default(),
            collision_shape_manager: Default::default(),
            collision_layer_manager: Default::default(),
            entity_draw_layers: Default::default(),
            tag_manager: Default::default(),
            snap_manager: Default::default(),
            piece_entity_manager: Default::default(),
            debug_entities: Default::default(),
            hidden_entities: Default::default(),
            no_collision_entities: Default::default(),
            pile_manager: Default::default(),
            piece_textures: Default::default(),
            entity_draw_order: Default::default(),
            entity_transforms: Default::default(),
            piece_colliders: Default::default(),
            quadtree: QuadTreeManager::new(quadtree_rect),
            snap_quadtree: QuadTreeManager::new(quadtree_rect),
            visible_rect,
            color: C::from_rgba(255, 255, 255, 255),

            lobby_rect: visible_rect,

            max_selector_scroll: 0.0,
        }
    }

    /// resets the board, clearing all data accept player positions
    /// background color is reset to white
    pub fn reset(&mut self) {
        self.player_manager.reset_player_states();

        self.texture_manager.clear();
        self.collision_shape_manager.clear();
        self.collision_layer_manager.clear();
        self.entity_draw_layers.clear();
        self.tag_manager.clear();
        self.snap_manager.clear();
        self.piece_entity_manager.clear();
        self.debug_entities.clear();
        self.hidden_entities.clear();
        self.no_collision_entities.clear();
        self.pile_manager.clear();
        self.piece_textures.clear();
        self.entity_draw_order.clear();
        self.entity_transforms.clear();
        self.piece_colliders.clear();
        self.quadtree.clear();
        self.snap_quadtree.clear();
        self.color = C::from_rgba(255, 255, 255, 255);
    }

    pub fn validate_shape_ir(&self, ir: &CollisionShapeIR<S>) -> Result<(), String> {
        if let CollisionShapeIR::Identifier(i) = ir {
            match &i {
                EntityIdentifier::Entity(e) => {
                    if !self.collision_shape_manager.contains(*e) {
                        return Err(format!("No collision shape with id {} exists.", e.id()));
                    }
                }
                EntityIdentifier::Name(n) => {
                    if !self.collision_shape_manager.contains_name(n) {
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
            return Err(format!("An item named '{name}' already exists."));
        }

        self.validate_texture(&ir.texture)?;

        for collider in &ir.colliders {
            self.validate_collider_ir(collider)?;
        }

        Ok(())
    }

    pub fn validate_pile_ir(&self, ir: &PileIR<V>) -> Result<(), String> {
        if let Some(name) = &ir.name
            && self.piece_entity_manager.is_name_used(&name)
        {
            return Err(format!("An item named '{name}' already exists."));
        }

        Ok(())
    }

    /// check if a piece or pile entity is valid for this manager
    /// if so, returns the resolved entity
    fn validate_entity(&self, piece: EntityIdentifier<PieceEntity>) -> Result<PieceEntity, String> {
        match piece {
            EntityIdentifier::Entity(e) => {
                if !self.piece_entity_manager.is_used(e) {
                    Err(format!("Entity ID {} is not in use.", e.id()))
                } else {
                    Ok(e)
                }
            }

            EntityIdentifier::Name(n) => {
                if let Some(e) = self.piece_entity_manager.get_by_name(&n) {
                    Ok(e)
                } else {
                    Err(format!("Entity with name {n} does not exist."))
                }
            }
        }
    }

    /// check if a piece entity is valid for this manager
    /// if so, returns the resolved entity
    /// returns an error if the entity is not tracked or not a piece (may be a pile)
    pub fn validate_piece(
        &self,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<PieceEntity, String> {
        let entity = self.validate_entity(piece)?;

        if self.pile_manager.is_pile(entity) {
            if let Some(n) = self.piece_entity_manager.get_name(entity) {
                Err(format!(
                    "ID {} ('{n}') corresponds to a pile, not a piece.",
                    entity.id()
                ))
            } else {
                Err(format!(
                    "ID {} corresponds to a pile, not a piece.",
                    entity.id()
                ))
            }
        } else {
            Ok(entity)
        }
    }

    /// check if a player is playing
    pub fn validate_player(&self, player: PlayerColor) -> Result<(), String> {
        if self.player_manager.is_playing(player) {
            Ok(())
        } else {
            return Err(format!("Player {player} does not exist."));
        }
    }

    /// check if a pile entity is valid for this manager
    /// if so, returns the resolved entity
    /// returns an error if the entity is not tracked or not a piece (may be a pile)
    pub fn validate_pile(
        &self,
        pile: EntityIdentifier<PieceEntity>,
    ) -> Result<PieceEntity, String> {
        let entity = self.validate_entity(pile)?;

        if !self.pile_manager.is_pile(entity) {
            if let Some(n) = self.piece_entity_manager.get_name(entity) {
                Err(format!(
                    "ID {} ('{n}') corresponds to a piece, not a pile.",
                    entity.id()
                ))
            } else {
                Err(format!(
                    "ID {} corresponds to a piece, not a pile.",
                    entity.id()
                ))
            }
        } else {
            Ok(entity)
        }
    }

    /// checks if a texture is valid
    pub fn validate_texture(&self, texture: &String) -> Result<(), String> {
        if !self.texture_manager.contains_texture_name(&texture) {
            Err(format!("No texture has name {texture}."))
        } else {
            Ok(())
        }
    }

    /// sets the texture of a piece
    pub fn set_texture(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        texture: &String,
    ) -> Result<(), String> {
        let piece = self.validate_piece(piece.clone())?;
        self.validate_texture(texture)?;

        self.set_piece_texture_unchecked(piece, texture);
        Ok(())
    }

    /// sets the texture of a piece without checking whether the or texture is valid
    fn set_piece_texture_unchecked(&mut self, piece: PieceEntity, texture: &String) {
        // determine texture entity
        let new_texture = self.texture_manager.get_texture_entity(texture).unwrap();

        // modify texture
        self.piece_textures.insert(piece, new_texture);
    }

    /// gets the transform of the item
    /// fails if the item does not exist
    pub fn get_transform(
        &mut self,
        entity: EntityIdentifier<PieceEntity>,
    ) -> Result<RigidTransform<V>, String> {
        let entity = self.validate_entity(entity)?;
        Ok(*self.entity_transforms.get(entity).unwrap())
    }

    /// sets the piece or piles transform
    pub fn set_transform(
        &mut self,
        entity: EntityIdentifier<PieceEntity>,
        transform: RigidTransform<V>,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.set_transform_unchecked(entity, transform);
        Ok(())
    }

    /// sets the piece or pile's position
    pub fn set_position(
        &mut self,
        entity: EntityIdentifier<PieceEntity>,
        position: V,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        let angle = self.entity_transforms.get(entity).unwrap().angle;

        let transform = RigidTransform { position, angle };

        self.set_transform_unchecked(entity, transform);
        Ok(())
    }

    /// sets the piece or pile's angle
    pub fn set_angle(
        &mut self,
        entity: EntityIdentifier<PieceEntity>,
        angle: f32,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        let position = self.entity_transforms.get(entity).unwrap().position;

        let transform = RigidTransform { position, angle };

        self.set_transform_unchecked(entity, transform);
        Ok(())
    }

    /// centers the item on the board
    pub fn center_item(&mut self, entity: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.center_entity_unchekced(entity);
        Ok(())
    }

    /// centers the item on the board
    /// does not check if the entity is valid
    fn center_entity_unchekced(&mut self, entity: PieceEntity) {
        let global_transform = self.get_entity_global_transform(entity);
        let center = self.dimensions().scaled(0.5);
        let delta = center.sub(global_transform.position);

        if let Some(parent) = self.pile_manager.get_containing_pile(entity) {
            let parent_transform = self.entity_transforms.get(parent).unwrap();
            self.set_pile_transform_unchecked(
                parent,
                RigidTransform {
                    position: parent_transform.position.add(delta),
                    angle: parent_transform.angle,
                },
            );
        } else {
            let original_transform = self.entity_transforms.get(entity).unwrap();
            self.set_transform_unchecked(
                entity,
                RigidTransform {
                    position: original_transform.position.add(delta),
                    angle: original_transform.angle,
                },
            );
        }
    }

    /// sets the transform of a pile or piece without checking whether the entity is valid
    /// if the entity being moved is snapping to something, only allows the angle to be modified
    fn set_transform_unchecked(&mut self, entity: PieceEntity, transform: RigidTransform<V>) {
        let transform = if let Some(player) = self.player_manager.get_holder(entity)
            && self.snap_manager.get_snap_target(player).is_some()
        {
            RigidTransform {
                position: self.entity_transforms.get(entity).unwrap().position,
                angle: transform.angle,
            }
        } else {
            transform
        };

        if self.pile_manager.is_pile(entity) {
            self.set_pile_transform_unchecked(entity, transform);
        } else {
            self.set_piece_transform_unchecked(entity, transform);
        }
    }

    /// sets the transform of a pile without checking whether the pile is valid
    fn set_pile_transform_unchecked(&mut self, pile: PieceEntity, transform: RigidTransform<V>) {
        // move pile
        self.entity_transforms.insert(pile, transform);
        self.update_pile_collisions_unchecked(pile);

        // move any items snapped to this one
        let snap_offset = self.pile_manager.get_snap_offset(pile).unwrap();
        self.set_snap_offset_unchecked(pile, snap_offset);
    }

    /// sets the snap radius of a pile
    pub fn set_snap_radius(
        &mut self,
        pile: EntityIdentifier<PieceEntity>,
        radius: f32,
    ) -> Result<(), String> {
        let pile = self.validate_pile(pile)?;

        self.pile_manager.set_snap_radius(pile, radius);
        let snap_position = self
            .entity_transforms
            .get(pile)
            .unwrap()
            .position
            .add(self.pile_manager.get_snap_offset(pile).unwrap());
        if radius > 0.0 {
            self.snap_quadtree.set_aabb(
                pile,
                R::from_center_dimension(snap_position, (radius * 2.0, radius * 2.0)),
            );
        } else {
            self.snap_quadtree.remove(pile);
        }

        Ok(())
    }

    /// gets the snap radius of a pile
    pub fn get_snap_radius(&mut self, pile: EntityIdentifier<PieceEntity>) -> Result<f32, String> {
        let pile = self.validate_pile(pile)?;
        Ok(self.pile_manager.get_snap_radius(pile).unwrap())
    }

    /// sets the snap offset of a pile
    pub fn set_snap_offset(
        &mut self,
        pile: EntityIdentifier<PieceEntity>,
        offset: V,
    ) -> Result<(), String> {
        let pile = self.validate_pile(pile)?;
        self.set_snap_offset_unchecked(pile, offset);
        Ok(())
    }

    /// gets the snap offset of a pile
    pub fn get_snap_offset(&mut self, pile: EntityIdentifier<PieceEntity>) -> Result<V, String> {
        let pile = self.validate_pile(pile)?;
        Ok(self.pile_manager.get_snap_offset(pile).unwrap())
    }

    /// sets the snap offset of a pile without checking whether the pile is valid
    /// also moves pieces snapped to this one
    fn set_snap_offset_unchecked(&mut self, pile: PieceEntity, offset: V) {
        self.pile_manager.set_snap_offset(pile, offset);

        let snap_position = self
            .entity_transforms
            .get(pile)
            .unwrap()
            .position
            .add(offset);
        if let Some(snapped_player_set) = self.snap_manager.get_snapped_players(pile) {
            let snapped_players: Vec<PlayerColor> = snapped_player_set.iter().collect();
            for player in snapped_players {
                self.set_position(
                    EntityIdentifier::Entity(self.player_manager.get_held_item(player).unwrap()),
                    snap_position,
                )
                .unwrap();
            }
        }
    }

    /// sets the transform of a piece without checking whether the piece is valid
    fn set_piece_transform_unchecked(&mut self, piece: PieceEntity, transform: RigidTransform<V>) {
        self.entity_transforms.insert(piece, transform);

        self.update_piece_collisions_unchecked(piece);
    }

    /// updates collisions for the given entity
    fn update_collisions_unchecked(&mut self, entity: PieceEntity) {
        if self.pile_manager.is_pile(entity) {
            self.update_pile_collisions_unchecked(entity);
        } else {
            self.update_piece_collisions_unchecked(entity);
        }
    }

    fn update_pile_collisions_unchecked(&mut self, pile: PieceEntity) {
        if self.is_entity_held(pile) {
            self.snap_quadtree.remove(pile);
            for piece in self.pile_manager.get_pieces_on_pile(pile) {
                self.quadtree.remove(piece);
            }
        } else {
            let (offset, radius) = self.pile_manager.get_snap_info(pile).unwrap();
            let snap_position = self
                .entity_transforms
                .get(pile)
                .unwrap()
                .position
                .add(offset);
            if radius > 0.0 {
                self.snap_quadtree.set_aabb(
                    pile,
                    R::from_center_dimension(snap_position, (radius * 2.0, radius * 2.0)),
                );
            }

            for piece in self.pile_manager.get_pieces_on_pile(pile) {
                if let Some(colliders) = self.piece_colliders.get(piece) {
                    let p_transform = self.get_entity_global_transform(piece);
                    let shape_iter = colliders
                        .iter()
                        .map(|s| self.get_collision_shape(s.shape).unwrap());
                    let aabb = geometry::compute_advanced_aabb(&p_transform, shape_iter).unwrap();

                    self.quadtree.set_aabb(piece, aabb);
                } else {
                    self.quadtree.remove(piece);
                }
            }
        }
    }

    fn update_piece_collisions_unchecked(&mut self, piece: PieceEntity) {
        if self.is_entity_held(piece) {
            self.quadtree.remove(piece);
        } else {
            if let Some(colliders) = self.piece_colliders.get(piece) {
                let p_transform = self.get_entity_global_transform(piece);
                let shape_iter = colliders
                    .iter()
                    .map(|s| self.get_collision_shape(s.shape).unwrap());
                let aabb = geometry::compute_advanced_aabb(&p_transform, shape_iter).unwrap();

                self.quadtree.set_aabb(piece, aabb);
            } else {
                self.quadtree.remove(piece);
            }
        }
    }

    /// sets the tags of an item
    pub fn set_tags<'a, 'b: 'a>(
        &'b mut self,
        entity: EntityIdentifier<PieceEntity>,
        tags: impl Iterator<Item = &'a String>,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        self.tag_manager.set_tags(entity, tags);

        Ok(())
    }

    /// adds tags to a piece
    pub fn add_tags<'a, 'b: 'a>(
        &'b mut self,
        entity: EntityIdentifier<PieceEntity>,
        tags: impl Iterator<Item = &'a String>,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        self.tag_manager.add_tags(entity, tags);

        Ok(())
    }

    /// removes tags from a piece
    pub fn remove_tags<'a, 'b: 'a>(
        &'b mut self,
        entity: EntityIdentifier<PieceEntity>,
        tags: impl Iterator<Item = &'a String>,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        self.tag_manager.remove_tags(entity, tags);
        Ok(())
    }

    /// clears the tags from a piece
    pub fn clear_tags(&mut self, entity: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        self.tag_manager.clear_tags(entity);
        Ok(())
    }

    /// checks if an entity has the given tag
    pub fn has_tag(
        &self,
        entity: EntityIdentifier<PieceEntity>,
        tag: &String,
    ) -> Result<bool, String> {
        let entity = self.validate_entity(entity)?;

        Ok(self.tag_manager.has_tag(entity, tag))
    }

    /// lists all entities with the given tag
    pub fn get_items_with_tag(&self, tag: &String) -> impl Iterator<Item = PieceEntity> {
        self.tag_manager.tag_pieces(tag)
    }

    /// sets the colliders of a piece
    /// fails if the piece does not exist
    pub fn set_colliders(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        collider_irs: Vec<PieceColliderIR<S>>,
    ) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;

        for collider in &collider_irs {
            self.validate_collider_ir(collider)?;
        }

        self.set_colliders_unchecked(piece, collider_irs);
        Ok(())
    }

    /// creates a new collision shape, returning its entity
    pub fn create_collision_shape(
        &mut self,
        name: String,
        shape: CollisionShape<S>,
    ) -> Result<CollisionShapeEntity, String> {
        if self.collision_shape_manager.contains_name(&name) {
            return Err(format!("CollisionShape with name '{name}' already exists."));
        }

        Ok(self.collision_shape_manager.add_named(name, shape).unwrap())
    }

    /// gets the collision shape referenced by the CollisionShapeEntity
    pub fn get_collision_shape(&self, shape: CollisionShapeEntity) -> Option<&CollisionShape<S>> {
        self.collision_shape_manager.get(shape)
    }

    /// gets the number of collision layers
    pub fn collision_layer_count(&self) -> usize {
        self.collision_layer_manager.len()
    }

    /// sets the colliders of a piece without checking whether the piece or collider list is valid
    /// (silently "fails") upon receiving an empty vector of colliders
    fn set_colliders_unchecked(
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
                        self.collision_shape_manager.get_entity(&n).unwrap()
                    }
                },
                CollisionShapeIR::Instance(val) => self.collision_shape_manager.add(val),
            };

            let layer_entity = self
                .collision_layer_manager
                .increment_count(&collider_ir.layer);

            let collider = PieceCollider {
                shape: shape_entity,
                layer: layer_entity,
            };
            colliders.push(collider);
        }
        if !colliders.is_empty() {
            // set colliders
            if let Some(colliders) = self.piece_colliders.insert(piece, colliders) {
                // decrement count of old colliders
                for collider in colliders {
                    self.collision_layer_manager.decrement_count(collider.layer);
                }
            }

            // update position based on new colliders
            if let Some(pile) = self.pile_manager.get_containing_pile(piece) {
                self.set_pile_transform_unchecked(pile, *self.entity_transforms.get(pile).unwrap());
            } else {
                self.set_piece_transform_unchecked(
                    piece,
                    *self.entity_transforms.get(piece).unwrap(),
                );
            }
        } else {
            // decrement count of old colliders
            if let Some(colliders) = self.piece_colliders.remove(piece) {
                for collider in colliders {
                    self.collision_layer_manager.decrement_count(collider.layer);
                }
            }
        }
    }

    pub fn create_piece(&mut self, ir: PieceIR<S, V>) -> Result<PieceEntity, String> {
        self.validate_piece_ir(&ir)?;

        let entity = if let Some(name) = ir.name {
            self.piece_entity_manager.get_new_named(name).unwrap()
        } else {
            self.piece_entity_manager.get_new()
        };

        self.set_piece_texture_unchecked(entity, &ir.texture);

        self.entity_draw_layers.set(entity, ir.draw_layer);

        self.entity_draw_order.push_back(entity);

        self.tag_manager.add_tags(entity, ir.tags.iter());

        self.set_piece_transform_unchecked(entity, ir.transform);

        self.set_colliders_unchecked(entity, ir.colliders);

        Ok(entity)
    }

    pub fn create_pile(&mut self, ir: PileIR<V>) -> Result<PieceEntity, String> {
        self.validate_pile_ir(&ir)?;

        let entity = if let Some(name) = ir.name {
            self.piece_entity_manager.get_new_named(name).unwrap()
        } else {
            self.piece_entity_manager.get_new()
        };

        self.pile_manager
            .create_pile(entity, ir.snap_radius, ir.snap_offset);

        self.entity_draw_layers.set(entity, ir.draw_layer);

        self.entity_draw_order.push_back(entity);

        self.tag_manager.add_tags(entity, ir.tags.iter());

        self.set_pile_transform_unchecked(entity, ir.transform);

        Ok(entity)
    }

    /// deletes an existing item
    pub fn delete_item(&mut self, entity: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        if self.pile_manager.is_pile(entity) {
            self.delete_pile_unchecked(entity);
        } else {
            self.delete_piece_unchecked(entity);
        }

        Ok(())
    }

    /// deletes an existing piece
    fn delete_piece_unchecked(&mut self, piece: PieceEntity) {
        if let Some(player) = self.player_manager.get_holder(piece) {
            self.drop_held_item_unchecked(player);
        }

        self.set_colliders_unchecked(piece, Vec::new());
        self.quadtree.remove(piece);
        self.tag_manager.clear_tags(piece);
        self.debug_entities.remove(piece);
        self.hidden_entities.remove(piece);
        self.no_collision_entities.remove(piece);
        self.entity_draw_layers.remove_piece(piece);
        self.piece_textures.remove(piece);
        self.entity_draw_order.remove(
            self.entity_draw_order
                .iter()
                .position(|p| *p == piece)
                .unwrap(),
        );
        self.entity_transforms.remove(piece);

        self.piece_entity_manager.mark_unused(piece);
    }

    /// deletes an existing pile (not the pieces in the pile)
    /// if the pile is held by a player, its contents are automatically dropped
    fn delete_pile_unchecked(&mut self, pile: PieceEntity) {
        if let Some(player) = self.player_manager.get_holder(pile) {
            self.drop_held_item_unchecked(player);
        }

        let transform = *self.entity_transforms.get(pile).unwrap();
        let draw_order_position = self
            .entity_draw_order
            .iter()
            .position(|p| *p == pile)
            .unwrap();

        // iterate backwards so we can keep inserting piece after the pile in draw order
        for piece in self.pile_manager.remove_pile(pile).into_iter().rev() {
            self.entity_draw_order
                .insert(draw_order_position + 1, piece);
            self.entity_transforms.insert(
                piece,
                transform.apply_to_other(self.entity_transforms.get(piece).unwrap()),
            );
        }

        self.snap_manager.remove_snap_target(pile);
        self.tag_manager.clear_tags(pile);
        self.debug_entities.remove(pile);
        self.hidden_entities.remove(pile);
        self.no_collision_entities.remove(pile);
        self.entity_draw_layers.remove_piece(pile);
        self.entity_draw_order.remove(draw_order_position);
        self.snap_quadtree.remove(pile);
        self.entity_transforms.remove(pile);

        self.piece_entity_manager.mark_unused(pile);
    }

    /// adds a piece to the top of the given pile
    pub fn pile_top(
        &mut self,
        pile: EntityIdentifier<PieceEntity>,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;
        let pile = self.validate_pile(pile)?;

        self.pile_top_unchecked(piece, pile);
        Ok(())
    }

    /// puts a piece on top of the given pile without checking if the piece or pile is valid
    fn pile_top_unchecked(&mut self, pile: PieceEntity, piece: PieceEntity) {
        // remove from existing pile
        self.unpile_entity(piece);

        // preserve global position
        let pile_transform = self.entity_transforms.get(pile).unwrap();
        let global_transform = self.entity_transforms.get(piece).unwrap();
        let transform = pile_transform.apply_inv_to_other(global_transform);

        // add to top of new pile
        self.pile_manager.push_top(pile, piece);
        self.set_piece_transform_unchecked(pile, transform);

        self.update_piece_collisions_unchecked(piece);

        self.entity_draw_order.remove(
            self.entity_draw_order
                .iter()
                .position(|p| *p == piece)
                .unwrap(),
        );
    }

    /// adds a piece to the bottom of the given pile
    pub fn pile_bottom(
        &mut self,
        pile: EntityIdentifier<PieceEntity>,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;
        let pile = self.validate_pile(pile)?;

        self.pile_bottom_unchecked(piece, pile);
        Ok(())
    }

    /// puts a piece on top of the given pile without checking if the piece or pile is valid
    fn pile_bottom_unchecked(&mut self, pile: PieceEntity, piece: PieceEntity) {
        // remove from existing pile
        self.unpile_entity(piece);

        // preserve global position
        let pile_transform = self.entity_transforms.get(pile).unwrap();
        let global_transform = self.entity_transforms.get(piece).unwrap();
        let transform = pile_transform.apply_inv_to_other(global_transform);

        // add to bottom of new pile
        self.pile_manager.push_bottom(pile, piece);
        self.set_piece_transform_unchecked(pile, transform);

        self.update_piece_collisions_unchecked(piece);

        self.entity_draw_order.remove(
            self.entity_draw_order
                .iter()
                .position(|p| *p == piece)
                .unwrap(),
        );
    }

    /// removes a piece from the pile its on
    pub fn unpile(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;
        self.unpile_entity(piece);
        Ok(())
    }

    /// removes a piece from the pile its on, if it is on a pile
    fn unpile_entity(&mut self, piece: PieceEntity) {
        let transform = self.get_entity_global_transform(piece);
        if let Some(pile) = self.pile_manager.unpile(piece) {
            // preserve global position
            self.entity_transforms.insert(piece, transform);
            let pile_idx = self
                .entity_draw_order
                .iter()
                .position(|p| *p == pile)
                .unwrap();

            // draw piece directly after the pile that used to contain it
            self.entity_draw_order.insert(pile_idx + 1, piece);

            // update collisions in case the pile was held
            self.update_piece_collisions_unchecked(piece);
        }
    }

    /// returns an iterator over the existing draw layers in sorted order
    pub fn draw_layer_iter(&self) -> impl Iterator<Item = i32> {
        self.entity_draw_layers.layer_iter()
    }

    /// returns the number of draw layers
    pub fn draw_layer_count(&self) -> usize {
        self.entity_draw_layers.len()
    }

    /// returns an iterator over texture-owning pieces and their transforms in the order in which
    /// they should be drawn (appearing first means should be on the bottom)
    pub fn texture_draw_iter(
        &self,
    ) -> impl Iterator<Item = (&T, Option<&[PieceCollider]>, RigidTransform<V>, i32)> {
        BoardDrawIter {
            board: &self,
            layered_entity_iter: self.entity_draw_order.iter().filter_map(|entity| {
                if self.is_hidden(*entity) || self.is_entity_held(*entity) {
                    None
                } else {
                    Some(*entity)
                }
            }),
            pile_iter: None,
            output_filter_map: |piece| {
                if self.is_hidden(piece) {
                    return None;
                }

                let texture_entity = self.piece_textures.get(piece).unwrap();

                // resource unwrap is safe because all piece_texture components are valid
                let texture = &self
                    .texture_manager
                    .get_texture(*texture_entity)
                    .unwrap()
                    .texture;
                let transform = self.get_entity_global_transform(piece);
                let draw_layer = self.get_draw_layer(piece).unwrap();
                let colliders = if self.debug_entities.contains(piece) {
                    self.piece_colliders.get(piece).map(|l| l.as_slice())
                } else {
                    None
                };

                Some((texture, colliders, transform, draw_layer))
            },
        }
    }

    /// returns an iterator over the textures of held pieces and their transforms
    /// held items should be drawn on top of all other pieces
    pub fn held_item_draw_iter(
        &self,
    ) -> impl Iterator<Item = (&T, Option<&[PieceCollider]>, RigidTransform<V>)> {
        BoardDrawIter {
            board: &self,
            layered_entity_iter: self
                .player_manager
                .held_item_iter()
                .filter(|entity| if self.is_hidden(*entity) { false } else { true }),
            pile_iter: None,
            output_filter_map: |piece| {
                if self.is_hidden(piece) {
                    return None;
                }

                let texture_entity = self.piece_textures.get(piece).unwrap();

                let texture = &self
                    .texture_manager
                    .get_texture(*texture_entity)
                    .unwrap()
                    .texture;
                let transform = self.get_entity_global_transform(piece);
                let colliders = if self.debug_entities.contains(piece) {
                    self.piece_colliders.get(piece).map(|l| l.as_slice())
                } else {
                    None
                };

                Some((texture, colliders, transform))
            },
        }
    }

    /// returns an iterator over debug-enabled snapping circles
    pub fn debug_snap_circle_draw_iter(&self) -> impl Iterator<Item = (V, f32)> {
        self.snap_quadtree.entity_iter().filter_map(|p| {
            if !self.debug_entities.contains(p) {
                return None;
            }
            if let Some((pos, radius)) = self.pile_manager.get_snap_info(p) {
                if radius <= 0.0 {
                    None
                } else {
                    Some((
                        self.get_entity_global_transform(p).position.add(pos),
                        radius,
                    ))
                }
            } else {
                None
            }
        })
    }

    /// returns an iterator over players, their positions, and their held pieces
    /// players should be drawn on top of every piece
    pub fn player_draw_iter<'a, 'b, F2>(
        &'b self,
        global_assets: &'a GlobalAssets<T, F2>,
    ) -> impl Iterator<Item = (&'a T, V, C)> {
        self.player_manager
            .position_iter()
            .filter_map(|(color, pos)| {
                if !self.player_manager.is_visible(color) {
                    return None;
                }

                let texture = if self.player_manager.is_holding(color) {
                    &global_assets.closed_hand_texture
                } else {
                    &global_assets.open_hand_texture
                };
                Some((texture, pos, color.into_color()))
            })
    }

    /// gets the dimensions of the board
    pub fn dimensions(&self) -> V {
        self.visible_rect.dimensions()
    }

    /// attempts to create a new texture, returning its entity on success
    /// textures must be assigned a name
    pub fn create_texture<IR>(
        &mut self,
        project_path: &Path,
        name: Option<String>,
        visual: &VisualInfo<C, R, V>,
        renderer: &mut IR,
    ) -> Result<TextureEntity, String>
    where
        C: IColor,
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        let magnification_dimensions = self.dimensions().scaled(Self::MAGNIFICATION_RATIO);
        Ok(self.texture_manager.load_texture(
            project_path,
            name,
            visual,
            magnification_dimensions,
            renderer,
        )?)
    }

    pub fn is_playing(&self, color: PlayerColor) -> bool {
        self.player_manager.is_playing(color)
    }

    pub fn add_player(&mut self, color: PlayerColor) -> Result<(), String> {
        self.player_manager.new_player(color)
    }

    pub fn remove_player(&mut self, color: PlayerColor) {
        if self.player_manager.is_playing(color) {
            self.drop_held_item_unchecked(color);
        }
        self.player_manager.remove_player(color);
    }

    /// gets the name of the texture that a piece contains
    pub fn get_texture(
        &self,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<Option<&String>, String> {
        let piece = self.validate_piece(piece)?;

        Ok(self
            .piece_textures
            .get(piece)
            .map(|i| self.texture_manager.get_texture_name(*i).unwrap()))
    }

    /// gets the texture by name
    pub fn get_texture_resource(&self, name: &String) -> Option<&T> {
        self.texture_manager
            .get_texture_by_name(name)
            .map(|t| &t.texture)
    }

    /// returns a lua table querying the given player for piles it can snap to, returning
    /// a sequential table containing the ids of all piles, in order of nearest to furthest
    /// from the player
    pub fn lua_query_snaps(&self, lua: &Lua, player: PlayerColor) -> Result<mlua::Table, String> {
        self.validate_player(player)?;

        let snaps = self.query_snaps(player);

        let table = lua
            .create_table_with_capacity(snaps.len(), 0)
            .expect("Unable to create snap query table.");

        for (idx, pile) in snaps.iter().enumerate() {
            table
                .raw_set(idx + 1, pile.id())
                .expect("Unable to create snap query table.");
        }

        Ok(table)
    }

    /// returns a vector the piles a player can snap to, in order of least to most distance from the
    /// snap point assumes the player is valid
    fn query_snaps(&self, player: PlayerColor) -> Vec<PieceEntity> {
        let position = self.player_manager.get_position(player).unwrap();

        // query position and filter for contained points
        let mut contained: Vec<PieceEntity> = self
            .snap_quadtree
            .query_point(position)
            .into_iter()
            .filter(|p| {
                let (offset, radius) = self.pile_manager.get_snap_info(*p).unwrap();
                let pile_position = self.get_entity_global_transform(*p).position.add(offset);
                pile_position.distance_to(position) <= radius
            })
            .collect();

        contained.sort_unstable_by_key(|p| {
            let offset = self.pile_manager.get_snap_offset(*p).unwrap();
            let pile_position = self.get_entity_global_transform(*p).position.add(offset);
            NumericF32::new(pile_position.distance_to(position))
        });

        contained
    }

    /// returns a lua table containing the collision layers colliding with the given item
    ///
    /// the output table contains two table fields: overlapping_layers and overlapping_pieces
    /// both contained tables are lists of overlapping layers/pieces
    /// layers are provided as collision layer names
    /// pieces are provided as piece ids
    pub fn lua_query_overlaps(
        &self,
        lua: &Lua,
        entity: EntityIdentifier<PieceEntity>,
    ) -> Result<mlua::Table, String> {
        let entity = self.validate_entity(entity)?;

        let (layers, pieces) = if self.pile_manager.is_pile(entity) {
            self.query_pile_overlaps(entity)
        } else {
            self.query_piece_overlaps(entity)
        };

        Ok(self.make_lua_overlap_query(lua, layers, pieces))
    }

    /// constructs a lua table with overlap information
    /// the output table contains two table fields: overlapping_layers and overlapping_pieces
    /// both contained tables are lists of overlapping layers/pieces
    /// layers are provided as collision layer names
    /// pieces are provided as piece ids
    fn make_lua_overlap_query(
        &self,
        lua: &Lua,
        layers: DenseSet<CollisionLayerEntity>,
        pieces: DenseSet<PieceEntity>,
    ) -> mlua::Table {
        let layer_table = lua
            .create_table_with_capacity(layers.len(), 0)
            .expect("Unable to create overlap query table.");

        let piece_table = lua
            .create_table_with_capacity(pieces.len(), 0)
            .expect("Unable to create overlap query table.");

        for (idx, layer) in layers.iter().enumerate() {
            let identifier: &str = &self.collision_layer_manager.get_name(layer).unwrap();
            layer_table
                .raw_set(idx + 1, identifier)
                .expect("Unable to create overlap query table.");
        }

        for (idx, piece) in pieces.iter().enumerate() {
            piece_table
                .raw_set(idx + 1, piece.id())
                .expect("Unable to create overlap query table.");
        }

        let table = lua
            .create_table_with_capacity(0, 2)
            .expect("Unable to create overlap query table.");

        table
            .raw_set("overlapping_layers", layer_table)
            .expect("Unable to create overlap query table.");
        table
            .raw_set("overlapping_pieces", piece_table)
            .expect("Unable to create overlap query table.");

        table
    }

    /// returns pieces overlapping with another piece and the pieces that are overlapping with
    fn query_pile_overlaps(
        &self,
        pile: PieceEntity,
    ) -> (DenseSet<CollisionLayerEntity>, DenseSet<PieceEntity>) {
        debug_assert!(self.pile_manager.is_pile(pile));

        if !self.should_collide(pile) {
            return (DenseSet::with_capacity(0), DenseSet::with_capacity(0));
        }

        let blacklist = {
            let mut blacklist = SparseSet::new();
            for piece in self.pile_manager.get_pieces_on_pile(pile) {
                blacklist.insert(piece);
            }
            blacklist
        };

        // the overlapping pieces
        let mut colliding_layers = DenseSet::new();
        let mut colliding_pieces = DenseSet::new();

        for piece in self.pile_manager.get_pieces_on_pile(pile) {
            let (p_layers, p_pieces) = self.query_piece_overlaps_blacklist(piece, &blacklist);
            colliding_layers.union(p_layers);
            colliding_pieces.union(p_pieces);
        }

        (colliding_layers, colliding_pieces)
    }

    fn query_piece_overlaps(
        &self,
        piece: PieceEntity,
    ) -> (DenseSet<CollisionLayerEntity>, DenseSet<PieceEntity>) {
        self.query_piece_overlaps_blacklist(piece, &SparseSet::new())
    }

    /// returns pieces overlapping with another piece and the pieces that are overlapping with
    /// blacklist is the set of pieces that won't be checked for overlaps
    fn query_piece_overlaps_blacklist(
        &self,
        piece: PieceEntity,
        blacklist: &SparseSet<PieceEntity>,
    ) -> (DenseSet<CollisionLayerEntity>, DenseSet<PieceEntity>) {
        debug_assert!(!self.pile_manager.is_pile(piece));

        let colliders = if let Some(c) = self.piece_colliders.get(piece)
            && self.should_collide(piece)
        {
            c
        } else {
            return (DenseSet::with_capacity(0), DenseSet::with_capacity(0));
        };

        // group colliders by layer
        let shapes_by_layer = {
            let mut out = DenseComponentManager::new();

            for collider in colliders.iter() {
                let shape_list = out.get_mut_or_insert(collider.layer, || Vec::new());
                let shape = self.collision_shape_manager.get(collider.shape).unwrap();
                shape_list.push(shape);
            }

            out
        };

        let transform = &self.get_entity_global_transform(piece);

        // can unwrap aabb because only non-empty collider lists are inserted into collider component
        let aabb = geometry::compute_advanced_aabb(
            transform,
            colliders
                .iter()
                .map(|c| self.collision_shape_manager.get(c.shape).unwrap()),
        )
        .unwrap();

        let mut colliding_layers = DenseSet::new();
        let mut colliding_pieces = DenseSet::new();

        'aabb_loop: for p in self.quadtree.query_rect(aabb) {
            if p == piece || blacklist.contains(p) || !self.should_collide(p) {
                continue 'aabb_loop;
            }

            // piece-associated data
            let p_colliders = self.piece_colliders.get(p).unwrap();
            let p_transform = &self.get_entity_global_transform(p);

            'collider_loop: for p_collider in p_colliders {
                // skip layers already checked
                if colliding_layers.contains(p_collider.layer) {
                    continue 'collider_loop;
                }

                // skip layers that the piece does not have
                let shapes = if let Some(s) = shapes_by_layer.get(p_collider.layer) {
                    s
                } else {
                    continue 'collider_loop;
                };

                // unwrap is safe because all collider ids are contained in collision shape resources
                let p_shape = self.collision_shape_manager.get(p_collider.shape).unwrap();

                'shape_loop: for shape in shapes {
                    if shape.intersects(transform, p_shape, p_transform) {
                        colliding_pieces.insert(p);
                        colliding_layers.insert(p_collider.layer);

                        break 'shape_loop;
                    }
                }
            }
        }

        (colliding_layers, colliding_pieces)
    }

    /// checks if a texture with the given name exists
    pub fn contains_texture(&self, name: &String) -> bool {
        self.texture_manager.contains_texture_name(name)
    }

    /// returns a list of pieces overlapping with the given point
    pub fn lua_query_point_overlaps(&mut self, lua: &Lua, position: V) -> mlua::Table {
        let pieces = self.query_point_overlaps(position);

        let table = lua
            .create_table_with_capacity(pieces.len(), 0)
            .expect("Unable to create point overlap query table.");

        for (idx, piece) in pieces.iter().enumerate() {
            table
                .raw_set(idx + 1, piece.id())
                .expect("Unable to create point overlap query table.");
        }

        table
    }

    /// returns a list of pieces overlapping with the given point
    pub fn query_point_overlaps(&mut self, position: V) -> Vec<PieceEntity> {
        // get pieces with overlapping aabbs
        let colliding_aabbs = self.quadtree.query_point(position);
        if colliding_aabbs.is_empty() {
            return Vec::new();
        }

        // filter out pieces that are aren't actually colliding
        colliding_aabbs
            .into_iter()
            .filter(|piece| {
                let transform = self.get_entity_global_transform(*piece);
                let shape_iter = self
                    .piece_colliders
                    .get(*piece)
                    .unwrap()
                    .iter()
                    .map(|collider| self.collision_shape_manager.get(collider.shape).unwrap());

                for shape in shape_iter {
                    if shape.contains_point(transform, position) {
                        return true;
                    }
                }
                false
            })
            .collect()
    }

    /// returns a list of pieces overlapping with the given point
    pub fn lua_query_top_piece(&mut self, position: V) -> mlua::Value {
        let piece_opt = self.query_top_piece(position);

        if let Some(piece) = piece_opt {
            mlua::Value::Integer(piece.id() as i64)
        } else {
            mlua::Value::Nil
        }
    }

    /// returns the visual top piece at the given point, excluding held or hidden pieces
    pub fn query_top_piece(&mut self, position: V) -> Option<PieceEntity> {
        // get pieces with overlapping aabbs
        let colliding_aabbs = self.quadtree.query_point(position);
        if colliding_aabbs.is_empty() {
            return None;
        }

        // filter out pieces that are invisible or aren't actually colliding
        let colliding = colliding_aabbs.into_iter().filter(|piece| {
            if self.is_hidden(*piece) {
                return false;
            }

            let transform = self.get_entity_global_transform(*piece);
            let shape_iter = self
                .piece_colliders
                .get(*piece)
                .unwrap()
                .iter()
                .map(|collider| self.collision_shape_manager.get(collider.shape).unwrap());

            for shape in shape_iter {
                if shape.contains_point(transform, position) {
                    return true;
                }
            }
            false
        });

        // find the highest draw layer
        // also if colliding is empty, return
        let max_draw_layer = colliding
            .clone()
            .map(|piece| self.get_draw_layer(piece).unwrap())
            .max()?;

        // place all draw layer-owning entities in the highest draw layer into a set
        // we will use these sets to find the highest piece
        // high_unpiled is the set of piles and non-piled pieces
        // high_pieces is the set of all pieces
        let mut high_unpiled = SparseSet::new();
        let mut high_pieces = SparseSet::new();
        for piece in
            colliding.filter(|piece| self.get_draw_layer(*piece).unwrap() == max_draw_layer)
        {
            if let Some(pile) = self.pile_manager.get_containing_pile(piece) {
                high_unpiled.insert(pile);
            } else {
                high_unpiled.insert(piece);
            }
            high_pieces.insert(piece);
        }

        // get and return the last piece drawn
        for entity in self.entity_draw_order.iter().copied().rev() {
            if high_unpiled.contains(entity) {
                return Some(if self.pile_manager.is_pile(entity) {
                    self.pile_manager
                        .get_pieces_on_pile(entity)
                        .rev()
                        .find(|p| high_pieces.contains(*p))
                        .unwrap()
                } else {
                    entity
                });
            }
        }

        None
    }

    /// makes a piece show its collision shapes
    pub fn show_debug(&mut self, entity: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.debug_entities.insert(entity);
        Ok(())
    }

    /// makes a piece hide its collision shapes
    pub fn hide_debug(&mut self, entity: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.debug_entities.remove(entity);
        Ok(())
    }

    /// checks if an item is a piece
    pub fn is_piece(&self, entity: EntityIdentifier<PieceEntity>) -> bool {
        let entity = match self.validate_entity(entity) {
            Ok(e) => e,
            Err(_) => return false,
        };

        !self.pile_manager.is_pile(entity)
    }

    /// checks if an item is on a pile
    pub fn is_on_pile(&self, entity: EntityIdentifier<PieceEntity>) -> bool {
        let entity = match self.validate_entity(entity) {
            Ok(e) => e,
            Err(_) => return false,
        };

        self.pile_manager.is_on_pile(entity)
    }

    /// gets the pile containing the given piece
    pub fn get_containing_pile(&self, piece: EntityIdentifier<PieceEntity>) -> Option<PieceEntity> {
        let piece = match self.validate_piece(piece) {
            Ok(e) => e,
            Err(_) => return None,
        };
        self.pile_manager.get_containing_pile(piece)
    }

    /// checks if an item is a pile
    pub fn is_pile(&self, entity: EntityIdentifier<PieceEntity>) -> bool {
        let entity = match self.validate_entity(entity) {
            Ok(e) => e,
            Err(_) => return false,
        };

        self.pile_manager.is_pile(entity)
    }

    /// returns the item that a player is holding
    pub fn get_held_item(&self, player: PlayerColor) -> Option<PieceEntity> {
        self.player_manager.get_held_item(player)
    }

    /// gets the player holding the given piece
    pub fn get_holder(&self, entity: EntityIdentifier<PieceEntity>) -> Option<PlayerColor> {
        let entity = match self.validate_entity(entity) {
            Ok(e) => e,
            Err(_) => return None,
        };

        self.player_manager.get_holder(entity)
    }

    /// checks if an item is held
    pub fn is_held(&self, entity: EntityIdentifier<PieceEntity>) -> bool {
        let entity = match self.validate_entity(entity) {
            Ok(e) => e,
            Err(_) => return false,
        };
        self.is_entity_held(entity)
    }

    /// checks if a player is holding an item
    pub fn is_holding(&self, player: PlayerColor) -> bool {
        self.player_manager.is_holding(player)
    }

    /// checks if an item is grabbed
    fn is_entity_held(&self, entity: PieceEntity) -> bool {
        let entity = if let Some(pile) = self.pile_manager.get_containing_pile(entity) {
            pile
        } else {
            entity
        };
        self.player_manager.is_held(entity)
    }

    /// checks if an item is hidden
    pub fn is_hidden(&self, entity: PieceEntity) -> bool {
        if let Some(player) = self.player_manager.get_holder(entity) {
            if !self.player_manager.is_visible(player) {
                return false;
            }
        }

        let parent_or_self = if let Some(pile) = self.pile_manager.get_containing_pile(entity) {
            pile
        } else {
            entity
        };
        self.hidden_entities.contains(parent_or_self) || self.hidden_entities.contains(entity)
    }

    /// checks if an item should collide
    pub fn should_collide(&self, entity: PieceEntity) -> bool {
        let parent_or_self = if let Some(pile) = self.pile_manager.get_containing_pile(entity) {
            pile
        } else {
            entity
        };
        !(self.no_collision_entities.contains(parent_or_self)
            || self.no_collision_entities.contains(entity))
    }

    /// gets the draw layer of the given entity
    pub fn get_draw_layer(&self, entity: PieceEntity) -> Option<i32> {
        let entity = if let Some(pile) = self.pile_manager.get_containing_pile(entity) {
            pile
        } else {
            entity
        };
        self.entity_draw_layers.get(entity)
    }

    /// gets the transform of the pile containing this entity, or the identity transform if no
    /// pile contains this piece
    fn get_parent_transform(&self, entity: PieceEntity) -> RigidTransform<V> {
        match self.pile_manager.get_containing_pile(entity) {
            Some(pile) => *self.entity_transforms.get(pile).unwrap(),
            None => RigidTransform::identity(),
        }
    }

    /// gets the global transform of the given entity
    /// does not check if the entity is valid
    fn get_entity_global_transform(&self, entity: PieceEntity) -> RigidTransform<V> {
        self.get_parent_transform(entity)
            .apply_to_other(self.entity_transforms.get(entity).unwrap())
    }

    /// sets the snap target for the given player
    pub fn set_snap_target(
        &mut self,
        player: PlayerColor,
        pile_opt: Option<EntityIdentifier<PieceEntity>>,
    ) -> Result<(), String> {
        self.validate_player(player)?;
        let pile_opt = if let Some(pile) = pile_opt {
            Some(self.validate_pile(pile)?)
        } else {
            None
        };
        if self.is_holding(player) {
            self.set_snap_target_unchecked(player, pile_opt);
        }

        Ok(())
    }

    /// sets the snap target for the given player
    /// assumes that
    ///  - the player is valid
    ///  - the pile is valid
    ///  - the player is holding something
    fn set_snap_target_unchecked(&mut self, player: PlayerColor, pile_opt: Option<PieceEntity>) {
        if let Some(pile) = pile_opt {
            // change piece position piece position
            self.snap_manager.set_snap_target(player, None); // allow transform to be modified by unsnapping first
            let held_item = self.player_manager.get_held_item(player).unwrap();
            let transform = RigidTransform {
                position: self
                    .entity_transforms
                    .get(pile)
                    .unwrap()
                    .position
                    .add(self.pile_manager.get_snap_offset(pile).unwrap()),
                angle: self.entity_transforms.get(held_item).unwrap().angle,
            };
            self.set_transform_unchecked(held_item, transform);

            self.snap_manager.set_snap_target(player, Some(pile));
        } else {
            self.snap_manager.set_snap_target(player, None);
        }
    }

    /// makes a player grab or release a piece
    /// if the player is holding something already, that item will be dropped
    /// fails if the player or entity to grab is invalid
    pub fn set_held_item(
        &mut self,
        player: PlayerColor,
        entity: Option<EntityIdentifier<PieceEntity>>,
    ) -> Result<(), String> {
        self.validate_player(player)?;

        // drop the existing item
        let _ = self.drop_held_item_unchecked(player);

        // grab the item
        if let Some(entity) = entity {
            let entity = self.validate_entity(entity)?;

            if self.pile_manager.is_pile(entity) {
                self.grab_pile_unchecked(player, entity);
            } else {
                self.grab_piece_unchecked(player, entity);
            }
        }

        Ok(())
    }

    /// makes the player grab a pile
    /// assumes that
    ///  - the player is valid
    ///  - the pile is valid
    ///  - the player is not holding anything
    fn grab_pile_unchecked(&mut self, player: PlayerColor, pile: PieceEntity) {
        if let Some(other_player) = self.player_manager.get_holder(pile) {
            // remove existing control of the pile
            self.player_manager.set_held_item(other_player, None);
        } else {
            // if pile is not yet held...

            // remove snap_circle from aabb
            self.snap_quadtree.remove(pile);

            // make sure to grab the pieces
            let mut piece_set = DenseSet::new();
            for piece in self.pile_manager.get_pieces_on_pile(pile) {
                self.quadtree.remove(piece);
                piece_set.insert(piece);
            }
            self.entity_draw_order.retain(|p| !piece_set.contains(*p));
        }

        // now grab the pile
        self.player_manager.set_held_item(player, Some(pile));

        // set pile position
        let transform = RigidTransform {
            position: self.player_manager.get_position(player).unwrap(),
            angle: self.entity_transforms.get(pile).unwrap().angle,
        };
        self.set_piece_transform_unchecked(pile, transform);
    }

    /// makes the player grab a piece
    /// assumes that
    ///  - the player is valid
    ///  - the piece is valid
    ///  - the player is not holding anything
    fn grab_piece_unchecked(&mut self, player: PlayerColor, piece: PieceEntity) {
        // remove piece from its pile, if its a part of one
        self.unpile_entity(piece);

        // update the piece to a grabbed state
        self.quadtree.remove(piece);
        self.player_manager.set_held_item(player, Some(piece));

        // set set piece position
        let transform = RigidTransform {
            position: self.player_manager.get_position(player).unwrap(),
            angle: self.entity_transforms.get(piece).unwrap().angle,
        };
        self.set_piece_transform_unchecked(piece, transform);
    }

    /// makes the player drop is held piece or pile
    /// fails if the player doesnt exist
    fn drop_held_item_unchecked(&mut self, player: PlayerColor) {
        if let Some(entity) = self.player_manager.get_held_item(player) {
            self.snap_manager.set_snap_target(player, None);

            self.player_manager.set_held_item(player, None);

            // put dropped piece at top of its draw layer
            let entity_idx = self
                .entity_draw_order
                .iter()
                .position(|p| *p == entity)
                .unwrap();
            self.entity_draw_order.remove(entity_idx);
            self.entity_draw_order.push_back(entity);

            // re-enable collisions
            self.update_collisions_unchecked(entity);
        }
    }

    /// gets the position of the player
    /// fails if the player does not exist
    pub fn get_player_position(&mut self, player: PlayerColor) -> Result<V, String> {
        self.validate_player(player)?;

        Ok(self.player_manager.get_position(player).unwrap())
    }

    /// moves the player by the given delta
    /// fails if the player does not exist
    pub fn move_player(&mut self, player: PlayerColor, delta: V) -> Result<(), String> {
        self.validate_player(player)?;

        let current_position = self.player_manager.get_position(player).unwrap();
        self.set_player_position(player, current_position.add(delta))
    }

    /// gets an iterator over all pieces (not piles)
    pub fn piece_iter(&self) -> impl Iterator<Item = PieceEntity> {
        self.piece_entity_manager
            .entity_iter()
            .filter(|e| !self.pile_manager.is_on_pile(*e))
    }

    /// gets an iterator over all piles (not pieces)
    pub fn pile_iter(&self) -> impl Iterator<Item = PieceEntity> {
        self.piece_entity_manager
            .entity_iter()
            .filter(|e| self.pile_manager.is_on_pile(*e))
    }

    /// gets an iterator over all items
    pub fn item_iter(&self) -> impl Iterator<Item = PieceEntity> {
        self.piece_entity_manager.entity_iter()
    }

    /// gets an iterator over all players
    pub fn player_iter(&self) -> impl Iterator<Item = PlayerColor> {
        self.player_manager.iter()
    }

    /// gets the number of players
    pub fn player_count(&self) -> usize {
        self.player_manager.len()
    }

    /// gets a lua list of all pieces
    pub fn lua_get_pieces(&self, lua: &Lua) -> mlua::Table {
        let table = lua
            .create_table_with_capacity(
                self.piece_entity_manager.len() - self.pile_manager.pile_count(),
                0,
            )
            .expect("unable to create all pieces table");

        for (idx, piece) in self.piece_iter().enumerate() {
            table
                .raw_set(idx + 1, piece.id())
                .expect("unable to create all pieces table");
        }

        table
    }

    /// gets a lua list of all piles
    pub fn lua_get_piles(&self, lua: &Lua) -> mlua::Table {
        let table = lua
            .create_table_with_capacity(self.pile_manager.pile_count(), 0)
            .expect("unable to create all piles table");

        for (idx, pile) in self.pile_iter().enumerate() {
            table
                .raw_set(idx + 1, pile.id())
                .expect("unable to create all piles table");
        }

        table
    }

    /// gets a lua list of all items
    pub fn lua_get_items(&self, lua: &Lua) -> mlua::Table {
        let table = lua
            .create_table_with_capacity(self.piece_entity_manager.len(), 0)
            .expect("unable to create all items table");

        for (idx, item) in self.item_iter().enumerate() {
            table
                .raw_set(idx + 1, item.id())
                .expect("unable to create all items table");
        }

        table
    }

    /// sets the position of the player
    /// fails if the player does not exist
    pub fn set_player_position(&mut self, player: PlayerColor, position: V) -> Result<(), String> {
        self.validate_player(player)?;

        // clamp position to board boundaries
        let position = self.visible_rect.fit_point(position);

        // update player position
        self.player_manager.set_position(player, position);

        // move held item if it's not snapping to something
        if let Some(entity) = self.player_manager.get_held_item(player)
            && self.snap_manager.get_snap_target(player).is_none()
        {
            self.set_position(EntityIdentifier::Entity(entity), position)
                .unwrap();
        }

        Ok(())
    }

    /// checks if the given player is visible
    pub fn is_player_visible(&self, player: PlayerColor) -> Result<bool, String> {
        self.validate_player(player)?;
        Ok(self.player_manager.is_visible(player))
    }

    /// makes a player visible
    pub fn show_player(&mut self, player: PlayerColor) -> Result<(), String> {
        self.validate_player(player)?;
        self.player_manager.show(player);
        Ok(())
    }

    /// makes a player hidden
    pub fn hide_player(&mut self, player: PlayerColor) -> Result<(), String> {
        self.validate_player(player)?;
        self.player_manager.hide(player);
        Ok(())
    }

    /// checks if the given item is visible
    pub fn is_visible(&self, entity: EntityIdentifier<PieceEntity>) -> Result<bool, String> {
        let entity = self.validate_entity(entity)?;
        let player_hidden = if let Some(player) = self.player_manager.get_holder(entity) {
            !self.player_manager.is_visible(player)
        } else {
            false
        };
        Ok(!self.hidden_entities.contains(entity) && !player_hidden)
    }

    /// makes an item visible
    pub fn show(&mut self, entity: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.hidden_entities.remove(entity);
        Ok(())
    }

    /// makes an item hidden
    pub fn hide(&mut self, entity: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.hidden_entities.insert(entity);
        Ok(())
    }

    /// disable a piece's collisions
    pub fn disable_collisions(
        &mut self,
        entity: EntityIdentifier<PieceEntity>,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.no_collision_entities.insert(entity);
        Ok(())
    }

    /// enable a piece's collisions
    pub fn enable_collisions(
        &mut self,
        entity: EntityIdentifier<PieceEntity>,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.no_collision_entities.remove(entity);
        Ok(())
    }

    // --------------------------------------------------------------------------------------------------
    // lobby board functionality ------------------------------------------------------------------------
    // --------------------------------------------------------------------------------------------------

    /// title height as a proportion of board height
    pub const UI_TITLE_RATIO: f32 = 0.07;

    /// background weight as a proportion of board shortest board dimension,
    pub const LOBBY_BACKGROUND_WEIGHT: f32 = 0.1;

    /// instructions width as a proportion of board width
    pub const LOBBY_INSTRUCTIONS_WIDTH_FACTOR: f32 = 0.35;

    /// remaining colors text height as a proportion of board height
    pub const LOBBY_REMAINING_COLORS_HEIGHT_FACTOR: f32 = 0.06;

    /// padding for content as a proportion of the minimum board dimension (width or height)
    pub const UI_PADDING_RATIO: f32 = 0.1;

    /// the max side length dimension of swatches
    pub const LOBBY_MAX_SWATCH_LENGTH: f32 = 150.0;

    /// the gap between swatches as a proportion of swatch length
    pub const LOBBY_SWATCH_GAP_FACTOR: f32 = 0.1;

    pub fn lobby_mode<IR>(&mut self, renderer: &mut IR, url: &str)
    where
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        self.reset();

        // generate assets based on board dimensions
        let board_rect = R::from_position_dimension(V::origin(), self.dimensions());
        let padding = (self.dimensions().y() * Self::UI_PADDING_RATIO).round();
        let [_, title_rect, _, content_rect, _] = board_rect.divide_proportions_vertical(
            &[
                Self::UI_PADDING_RATIO,
                Self::UI_TITLE_RATIO,
                Self::UI_PADDING_RATIO,
                1.0_f32 - Self::UI_TITLE_RATIO - 3.0 * Self::UI_PADDING_RATIO,
                Self::UI_PADDING_RATIO,
            ],
            0.0,
        );
        let title_rect = title_rect.expand_horizontal(-padding);
        let [_, instructions_rect, _, swatch_content_rect, _] = content_rect
            .divide_proportions_horizontal(
                &[
                    Self::UI_PADDING_RATIO,
                    Self::LOBBY_INSTRUCTIONS_WIDTH_FACTOR,
                    Self::UI_PADDING_RATIO,
                    1.0_f32 - Self::LOBBY_INSTRUCTIONS_WIDTH_FACTOR - 3.0 * Self::UI_PADDING_RATIO,
                    Self::UI_PADDING_RATIO,
                ],
                0.0,
            );
        let [remaining_colors_rect, _, swatches_rect] = swatch_content_rect
            .divide_proportions_vertical(
                &[
                    Self::LOBBY_REMAINING_COLORS_HEIGHT_FACTOR,
                    Self::LOBBY_REMAINING_COLORS_HEIGHT_FACTOR,
                    1.0 - Self::LOBBY_REMAINING_COLORS_HEIGHT_FACTOR * 4.0,
                ],
                0.0,
            );

        // make title
        let title_ir: VisualInfo<C, R, V> = VisualInfo::from(VisualComponent::from(TextboxInfo {
            rect: title_rect.with_position(V::origin()),
            text: format!("Waiting for players to join..."),
            align: crate::intermediate_representations::TextAlign::CenterCenter,
            spacing: None,
            size: None,
            font: String::from("ptserif-bold.ttf"),
            color: C::try_from_hex_str("000000").unwrap(),
        }));
        if let Err(msg) = self.create_texture(
            &PathBuf::new(),
            Some(String::from("title")),
            &title_ir,
            renderer,
        ) {
            panic!("Could not load lobby title texture: {msg}");
        }
        if let Err(msg) = self.create_piece(PieceIR {
            name: None,
            texture: String::from("title"),
            transform: title_rect.position::<V>().into(),
            colliders: Vec::new(),
            draw_layer: -1,
            tags: Vec::new(),
        }) {
            panic!("Could not load lobby title piece: {msg}");
        }

        // make instructions
        let instructions_ir: VisualInfo<C, R, V> = VisualInfo::from(VisualComponent::from(
            TextboxInfo {
                rect: instructions_rect.with_position(V::origin()),
                text: format!(
                    "1) Connect to this device's hotspot\n\n2) Open your web browser\n\n3) In the address bar, enter\n     the following URL:\n     {url}\n\n4) Select a color to play as\n\n 5) Use your screen as a touchpad"
                ),
                align: crate::intermediate_representations::TextAlign::TopLeft,
                spacing: None,
                size: None,
                font: String::from("ptserif-bold.ttf"),
                color: C::try_from_hex_str("000000").unwrap(),
            },
        ));
        if let Err(msg) = self.create_texture(
            &PathBuf::new(),
            Some(String::from("instructions")),
            &instructions_ir,
            renderer,
        ) {
            panic!("Could not load lobby instructions texture: {msg}");
        }
        if let Err(msg) = self.create_piece(PieceIR {
            name: None,
            texture: String::from("instructions"),
            transform: instructions_rect.position::<V>().into(),
            colliders: Vec::new(),
            draw_layer: -1,
            tags: Vec::new(),
        }) {
            panic!("Could not load lobby instructions piece: {msg}");
        }

        // make remaining colors text
        let remaining_ir: VisualInfo<C, R, V> =
            VisualInfo::from(VisualComponent::from(TextboxInfo {
                rect: remaining_colors_rect.with_position(V::origin()),
                text: format!("Colors remaining:"),
                align: crate::intermediate_representations::TextAlign::TopCenter,
                spacing: None,
                size: None,
                font: String::from("ptserif-bold.ttf"),
                color: C::try_from_hex_str("000000").unwrap(),
            }));
        if let Err(msg) = self.create_texture(
            &PathBuf::new(),
            Some(String::from("remaining")),
            &remaining_ir,
            renderer,
        ) {
            panic!("Could not load lobby remaining colors texture: {msg}");
        }
        if let Err(msg) = self.create_piece(PieceIR {
            name: None,
            texture: String::from("remaining"),
            transform: remaining_colors_rect.position::<V>().into(),
            colliders: Vec::new(),
            draw_layer: -1,
            tags: Vec::new(),
        }) {
            panic!("Could not load lobby remaining colors piece: {msg}");
        }

        // make background
        self.ui_create_background(renderer);

        // make swatches
        let major_axis_length = swatches_rect.width().max(swatches_rect.height());
        let minor_axis_length = swatches_rect.width().min(swatches_rect.height());
        let swatch_side_length = f32::round(
            (major_axis_length / (4.0 + Self::LOBBY_SWATCH_GAP_FACTOR * 3.0))
                .min(minor_axis_length / (3.0 + Self::LOBBY_SWATCH_GAP_FACTOR * 2.0))
                .min(Self::LOBBY_MAX_SWATCH_LENGTH)
                / 2.0,
        ) * 2.0;
        let swatch_gap = f32::floor(swatch_side_length * Self::LOBBY_SWATCH_GAP_FACTOR);
        let swatch_dimensions = V::from_xy(swatch_side_length, swatch_side_length);
        self.create_collision_shape(
            String::from("swatch-collider"),
            CollisionShape::from(S::from(R::from_center_dimension(
                V::origin(),
                swatch_dimensions,
            ))),
        )
        .expect("Unable to create swatch collision shape.");

        let swatch_position_iter = {
            let (start, rows, cols) = if major_axis_length == swatches_rect.width() {
                (
                    R::from_center_dimension(
                        swatches_rect.center::<V>(),
                        V::from_xy(
                            swatch_side_length * 4.0 + swatch_gap * 3.0,
                            swatch_side_length * 3.0 + swatch_gap * 2.0,
                        ),
                    )
                    .with_y(swatches_rect.y()),
                    3,
                    4,
                )
            } else {
                (
                    R::from_center_dimension(
                        swatches_rect.center::<V>(),
                        V::from_xy(
                            swatch_side_length * 3.0 + swatch_gap * 2.0,
                            swatch_side_length * 4.0 + swatch_gap * 3.0,
                        ),
                    )
                    .with_y(swatches_rect.y()),
                    4,
                    3,
                )
            };

            let cell_count = rows * cols;

            let mut index = 0;
            let x = start.x();
            let y = start.y();

            std::iter::from_fn(move || {
                if index >= cell_count {
                    return None;
                }

                let row = index / cols;
                let col = index % cols;
                index += 1;

                Some(
                    V::from_xy(
                        x + col as f32 * (swatch_side_length + swatch_gap),
                        y + row as f32 * (swatch_side_length + swatch_gap),
                    )
                    .add(swatch_dimensions.scaled(0.5)),
                )
            })
        };
        let color_iter = PlayerColor::LIST.iter().cloned();

        for (color, position) in color_iter.zip(swatch_position_iter) {
            self.lobby_create_swatch_texture(color, swatch_dimensions, renderer);
            if self.is_playing(color) {
                self.player_manager.set_position(color, position);
            } else {
                self.lobby_create_swatch_piece(color, position);
            }
        }

        self.lobby_rect = R::from_min_max(
            (
                instructions_rect.max::<V>().x() + swatch_side_length * 0.5,
                0.0,
            ),
            board_rect.max::<V>(),
        );
    }

    fn lobby_create_swatch_texture<IR>(
        &mut self,
        color: PlayerColor,
        dimensions: V,
        renderer: &mut IR,
    ) where
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        let swatch_ir: VisualInfo<C, R, V> = VisualInfo {
            components: vec![
                PatternInfo {
                    rect: R::from_center_dimension(V::origin(), dimensions),
                    pattern: crate::intermediate_representations::PatternType::Solid,
                    weight: 1.0,
                    spacing: 0.0,
                    angle: 0.0,
                    offset: V::origin(),
                    pattern_color: C::try_from_hex_str("000000").unwrap(),
                    background_color: C::try_from_hex_str("000000").unwrap(),
                }
                .into(),
                PatternInfo {
                    rect: R::from_center_dimension(V::origin(), dimensions).expand(-2.0),
                    pattern: crate::intermediate_representations::PatternType::Solid,
                    weight: 1.0,
                    spacing: 0.0,
                    angle: 0.0,
                    offset: V::origin(),
                    pattern_color: color.into_color::<C>(),
                    background_color: color.into_color::<C>(),
                }
                .into(),
            ],
        };
        if let Err(msg) = self.create_texture(
            &PathBuf::new(),
            Some(format!("swatch-{}", color.name())),
            &swatch_ir,
            renderer,
        ) {
            panic!("Could not load swatch texture: {msg}");
        }
    }

    fn lobby_create_swatch_piece(&mut self, color: PlayerColor, position: V) {
        if let Err(msg) = self.create_piece(PieceIR {
            name: Some(format!("swatch-{}", color.name())),
            texture: format!("swatch-{}", color.name()),
            transform: position.into(),
            colliders: vec![PieceColliderIR {
                shape: String::from("swatch-collider").into(),
                layer: String::from("default"),
            }],
            draw_layer: 0,
            tags: Vec::new(),
        }) {
            panic!("Could not load swatch piece: {msg}");
        }
    }

    /// adds a player, does nothing if the player already exists
    pub fn lobby_add_player(&mut self, color: PlayerColor) {
        if self.is_playing(color) {
            return;
        }

        let swatch_identifier = EntityIdentifier::Name(format!("swatch-{}", color.name()));

        self.add_player(color)
            .expect("Unable to add player: already exists.");
        let position = self
            .get_transform(swatch_identifier.clone())
            .expect("Unable to add player: missing swatch.")
            .position;
        self.set_player_position(color, position).unwrap();
        self.delete_item(swatch_identifier)
            .expect("Unable to add player: failed to delete swatch.");
    }

    /// removes a player, does nothing if the player does not exist
    pub fn lobby_remove_player(&mut self, color: PlayerColor) {
        if !self.is_playing(color) {
            return;
        }

        let position = self
            .get_player_position(color)
            .expect("Failed to remove player: couldn't get player position.");
        self.lobby_create_swatch_piece(color, position);
        self.remove_player(color);
    }

    /// handles a tap event for a player, does nothing if the player does not exist
    fn lobby_handle_tap(&mut self, color: PlayerColor) {
        if self.is_holding(color) {
            self.set_held_item(color, None)
                .expect("unable to drop item.");
        } else {
            let player_pos = self.get_player_position(color).unwrap();
            if let Some(piece) = self.query_top_piece(player_pos) {
                self.set_held_item(color, Some(EntityIdentifier::Entity(piece)))
                    .expect("unable to pick up item");
            }
        }
    }

    /// handles a movement event for a player, does nothing if the player does not exist
    fn lobby_handle_move(&mut self, color: PlayerColor, delta: V) {
        if !self.is_playing(color) {
            return;
        }
        let _ = self.set_player_position(
            color,
            self.lobby_rect
                .fit_point(self.player_manager.get_position(color).unwrap().add(delta)),
        );
    }

    /// handles a "raw" input event
    /// ignores invalid input events
    fn lobby_handle_input(&mut self, input: PlayerInputEvent) {
        let player = input.player;
        if !self.is_playing(player) {
            return;
        }
        match input.event {
            InputEvent::TrackpadDelta { delta_x, delta_y } => {
                self.lobby_handle_move(player, V::from_xy(delta_x, delta_y))
            }
            InputEvent::TrackpadPress => self.lobby_handle_tap(player),

            _ => (),
        }
    }

    /// handles input events in an iterator
    pub fn lobby_handle_inputs(&mut self, inputs: impl Iterator<Item = PlayerInputEvent>) {
        for input in inputs {
            self.lobby_handle_input(input);
        }
    }

    // --------------------------------------------------------------------------------------------------
    // game selector board functionality ----------------------------------------------------------------
    // --------------------------------------------------------------------------------------------------

    pub const SELECTOR_ENTRY_COLUMNS: u32 = 3;
    pub const SELECTOR_ENTRY_ASPECT_RATIO: f32 = 4.0 / 3.0;

    /// enters game selector mode
    /// with the given player color having sole control over inputs
    /// returns a map from piece entities to the game paths
    pub fn game_selector_mode<IR>(
        &mut self,
        renderer: &mut IR,
        host: PlayerColor,
        game_list: impl Iterator<Item = GameEntry<T>>,
    ) where
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        // setup board state
        self.reset();
        for player in self.player_manager.iter().collect::<Vec<PlayerColor>>() {
            self.player_manager.hide(player);
        }
        self.player_manager.show(host);
        self.player_manager
            .set_position(host, self.dimensions().scaled(0.5));

        // generate assets based on board dimensions
        let padding = (self.dimensions().y() * Self::UI_PADDING_RATIO).round();

        // generate background
        self.ui_create_background(renderer);

        // generate moving pile for scrolling
        let pile_ir = PileIR {
            name: Some("scroller".into()),
            transform: V::origin().into(),
            snap_offset: V::origin(),
            snap_radius: 0.0,
            draw_layer: 0,
            tags: Vec::new(),
        };
        let pile = match self.create_pile(pile_ir) {
            Ok(p) => p,
            Err(msg) => panic!("Could not create selector scroll pile: {msg}"),
        };

        // generate title
        let title_height = self.dimensions().y() * Self::UI_TITLE_RATIO;
        let title_texture_ir = VisualInfo {
            components: vec![
                TextboxInfo {
                    rect: R::from_position_dimension(
                        V::from_xy(0.0, padding),
                        V::from_xy(self.dimensions().x(), title_height),
                    ),
                    text: String::from("Select a Game"),
                    font: String::from("ptserif.ttf"),
                    align: crate::intermediate_representations::TextAlign::CenterCenter,
                    size: None,
                    spacing: None,
                    color: C::try_from_hex_str("000000").unwrap(),
                }
                .into(),
            ],
        };
        let title_texture =
            match self.create_texture(&PathBuf::new(), None, &title_texture_ir, renderer) {
                Ok(t) => t,
                Err(msg) => panic!("Failed to create selector title texture: {msg}"),
            };
        let title_piece_ir = PieceIR {
            name: None,
            texture: "background".into(),
            transform: V::origin().into(),
            colliders: Vec::new(),
            draw_layer: -5,
            tags: Vec::new(),
        };
        let title_piece = match self.create_piece(title_piece_ir) {
            Ok(p) => p,
            Err(msg) => panic!("Failed to create selector title piece: {msg}"),
        };
        self.piece_textures.insert(title_piece, title_texture);
        self.pile_top_unchecked(pile, title_piece);

        // generate entities for each entry
        let entry_dimensions = {
            let width =
                (self.dimensions().x() - padding * (Self::SELECTOR_ENTRY_COLUMNS + 1) as f32) / 3.0;
            let height = width / Self::SELECTOR_ENTRY_ASPECT_RATIO;
            V::from_xy(width, height)
        };
        self.create_collision_shape(
            String::from("entry"),
            CollisionShape(vec![S::from(R::from_center_dimension(
                V::origin(),
                entry_dimensions,
            ))]),
        )
        .expect("unable to create entry collision shape");
        let mut position_iter = Self::selector_entry_position_iter(padding, entry_dimensions)
            .map(|p| p.change_y(padding + title_height));
        let first_entry_y = (entry_dimensions.y() * 0.5) + padding;
        let mut furthest_entry_y = first_entry_y;
        for entry in game_list {
            let position = position_iter.next().unwrap();
            let entity = self.selector_create_entry_entity(renderer, entry, entry_dimensions);

            self.pile_top_unchecked(pile, entity);
            self.set_piece_transform_unchecked(entity, position.into());

            furthest_entry_y = position.y();
        }
        self.max_selector_scroll = furthest_entry_y - first_entry_y - self.dimensions().y()
            + padding * 2.0
            + entry_dimensions.y();

        // create tap again ui piece
        let tap_again_texture_ir = VisualInfo {
            components: vec![
                PatternInfo {
                    rect: R::from_center_dimension(V::origin(), entry_dimensions).expand(8.0),
                    pattern: PatternType::Solid,
                    angle: 0.0,
                    weight: 0.0,
                    offset: V::origin(),
                    spacing: 0.0,
                    pattern_color: C::try_from_hex_str("90DDDD").unwrap(),
                    background_color: C::try_from_hex_str("000000").unwrap(),
                }
                .into(),
            ],
        };
        let tap_again_texture =
            match self.create_texture(&PathBuf::new(), None, &tap_again_texture_ir, renderer) {
                Ok(t) => t,
                Err(msg) => panic!("Failed to create tap again texture: {msg}"),
            };
        let tap_again_piece_ir = PieceIR {
            name: Some("tap again".into()),
            texture: "background".into(),
            transform: V::origin().into(),
            colliders: Vec::new(),
            draw_layer: -5,
            tags: Vec::new(),
        };
        let tap_again_piece = match self.create_piece(tap_again_piece_ir) {
            Ok(p) => p,
            Err(msg) => panic!("Failed to create tap again piece: {msg}"),
        };
        self.piece_textures
            .insert(tap_again_piece, tap_again_texture);
        self.hidden_entities.insert(tap_again_piece);
        self.pile_bottom_unchecked(pile, tap_again_piece);
    }

    /// handles inputs in selector mode, does nothing if the player is not the host
    /// returns the path of the selected game, if any
    fn selector_handle_tap(&mut self, color: PlayerColor) -> Option<PathBuf> {
        if !self.player_manager.is_visible(color) {
            return None;
        }

        let player_pos = self.get_player_position(color).unwrap();

        // the only pieces with hitboxes are entries, and they don't overlap
        // so if we have a piece, it's an entry
        if let Some(piece) = self.query_top_piece(player_pos) {
            // we use the null byte as a sentenial value to mark an entry as selected
            // null byte is not allowed in paths for any operating system so we have no
            // chance of accidentally shadowing an entry directory
            if self.tag_manager.has_tag(piece, &"\0".to_string()) {
                return Some(
                    self.tag_manager
                        .piece_tags(piece)
                        .filter(|tag| tag.as_str() != "\0")
                        .next()
                        .unwrap()
                        .into(),
                );
            }

            let pos = self.entity_transforms.get(piece).unwrap();
            let tap_again = self
                .piece_entity_manager
                .get_by_name(&String::from("tap again"))
                .unwrap();
            self.set_piece_transform_unchecked(tap_again, *pos);
            self.hidden_entities.remove(tap_again);

            // remove null byte marker from previous entry
            let prev_piece_opt = self.tag_manager.tag_pieces(&"\0".to_string()).next();
            if let Some(prev_piece) = prev_piece_opt {
                self.tag_manager
                    .remove_tags(prev_piece, ["\0".to_string()].iter());
            }

            // add null byte marker to this entry
            self.tag_manager.add_tags(piece, ["\0".to_string()].iter());
        }

        None
    }

    /// handles a movement event for a player, does nothing if the player does not exist
    fn selector_handle_move(&mut self, color: PlayerColor, delta: V) {
        if !self.player_manager.is_visible(color) {
            return;
        }

        // handle scrolling
        let current_position = self.player_manager.get_position(color).unwrap();
        if current_position.y() >= self.dimensions().y() - 1.0 && delta.y() > 0.0
            || current_position.y() <= 1.0 && delta.y() < 0.0
        {
            let pile = self
                .piece_entity_manager
                .get_by_name(&"scroller".into())
                .unwrap();

            let init_transform = self.entity_transforms.get(pile).unwrap();
            let init_scroll = -init_transform.position.y();
            let new_scroll = (init_scroll + delta.y()).clamp(0.0, self.max_selector_scroll);

            if init_scroll != new_scroll {
                self.set_pile_transform_unchecked(
                    pile,
                    RigidTransform {
                        position: init_transform.position.with_y(-new_scroll),
                        angle: 0.0,
                    },
                );
            }
        }

        let _ = self.set_player_position(color, current_position.add(delta));

        // remove null byte marker from existing entry if necessary
        let prev_piece_opt = self.tag_manager.tag_pieces(&"\0".to_string()).next();
        if let Some(prev_piece) = prev_piece_opt {
            if self.query_top_piece(self.player_manager.get_position(color).unwrap())
                != Some(prev_piece)
            {
                let _ = self.hide(EntityIdentifier::Name("tap again".to_string()));
                self.tag_manager
                    .remove_tags(prev_piece, ["\0".to_string()].iter());
            }
        }
    }

    /// handles a "raw" input event
    /// ignores invalid input events
    fn selector_handle_input(&mut self, input: PlayerInputEvent) -> Option<PathBuf> {
        let player = input.player;
        if !self.is_playing(player) {
            return None;
        }
        match input.event {
            InputEvent::TrackpadDelta { delta_x, delta_y } => {
                self.selector_handle_move(player, V::from_xy(delta_x, delta_y));
                None
            }
            InputEvent::TrackpadPress => self.selector_handle_tap(player),

            _ => None,
        }
    }

    /// handles input events in an iterator
    pub fn selector_handle_inputs(
        &mut self,
        inputs: impl Iterator<Item = PlayerInputEvent>,
    ) -> Option<PathBuf> {
        let mut path = None;
        for input in inputs {
            let path_opt = self.selector_handle_input(input);
            if path_opt.is_some() {
                path = path_opt
            }
        }
        path
    }

    /// gets an endless iterator over entry positions
    fn selector_entry_position_iter(padding: f32, entry_dimensions: V) -> impl Iterator<Item = V> {
        let start = entry_dimensions.scaled(0.5).add((padding, padding));
        let mut col = 0;
        let mut cursor = start;

        std::iter::from_fn(move || {
            if col >= Self::SELECTOR_ENTRY_COLUMNS {
                cursor = cursor
                    .with_x(start.x())
                    .change_y(entry_dimensions.y() + padding);
                col = 0;
            }
            let out = cursor;
            cursor = cursor.change_x(entry_dimensions.x() + padding);
            col += 1;
            Some(out)
        })
    }

    /// creates a background texture for selector or lobby mode
    fn ui_create_background<IR>(&mut self, renderer: &mut IR)
    where
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        let board_rect = R::from_position_dimension(V::origin(), self.dimensions());

        let background_ir: VisualInfo<C, R, V> =
            VisualInfo::from(VisualComponent::from(PatternInfo {
                rect: board_rect.with_center(V::origin()),
                pattern: crate::intermediate_representations::PatternType::Checkers,
                pattern_color: C::try_from_hex_str("E8E8E8").unwrap(),
                background_color: C::try_from_hex_str("E0E0E0").unwrap(),
                offset: V::origin(),
                spacing: 0.0,
                angle: 45.0,
                weight: Self::LOBBY_BACKGROUND_WEIGHT * self.dimensions().y(),
            }));
        if let Err(msg) = self.create_texture(
            &PathBuf::new(),
            Some(String::from("background")),
            &background_ir,
            renderer,
        ) {
            panic!("Could not load ui background texture: {msg}");
        }
        if let Err(msg) = self.create_piece(PieceIR {
            name: None,
            texture: String::from("background"),
            transform: board_rect.center::<V>().into(),
            colliders: Vec::new(),
            draw_layer: -10,
            tags: Vec::new(),
        }) {
            panic!("Could not load ui background piece: {msg}");
        }
    }

    /// generates an entity for an entry, including a title and thumbnail
    fn selector_create_entry_entity<IR>(
        &mut self,
        renderer: &mut IR,
        entry: GameEntry<T>,
        dimensions: V,
    ) -> PieceEntity
    where
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        let rect = R::from_center_dimension(V::origin(), dimensions);
        let spacing = rect.height() * 0.05;
        let content_rect = rect.expand(-spacing);
        let [thumbnail_rect, title_rect] =
            content_rect.divide_proportions_vertical(&[0.8, 0.2], spacing);

        let mut components = Vec::new();
        components.push(
            PatternInfo {
                rect,
                pattern: PatternType::Solid,
                angle: 0.0,
                weight: 0.0,
                offset: V::origin(),
                spacing: 0.0,
                pattern_color: C::try_from_hex_str("B0B0B0").unwrap(),
                background_color: C::try_from_hex_str("B0B0B0").unwrap(),
            }
            .into(),
        );
        components.push(
            PatternInfo {
                rect: thumbnail_rect,
                pattern: PatternType::Solid,
                angle: 0.0,
                weight: 0.0,
                offset: V::origin(),
                spacing: 0.0,
                pattern_color: C::try_from_hex_str("000000").unwrap(),
                background_color: C::try_from_hex_str("000000").unwrap(),
            }
            .into(),
        );
        if let Some(thumbnail) = entry.thumbnail {
            self.texture_manager
                .add_image("thumbnail".to_owned(), thumbnail);
            components.push(
                ImageInfo {
                    position_data: ImagePosition::FitRect(thumbnail_rect),
                    path: String::from("thumbnail"),
                    tint: C::try_from_hex_str("ffffff").unwrap(),
                }
                .into(),
            );
        }
        components.push(
            TextboxInfo {
                rect: title_rect,
                text: entry.title.clone(),
                font: String::from("ptserif.ttf"),
                align: crate::intermediate_representations::TextAlign::CenterCenter,
                size: None,
                spacing: None,
                color: C::try_from_hex_str("000000").unwrap(),
            }
            .into(),
        );
        let texture_ir = VisualInfo { components };
        let texture = match self.create_texture(&PathBuf::new(), None, &texture_ir, renderer) {
            Ok(t) => t,
            Err(msg) => panic!("Failed to create entry texture: {msg}"),
        };
        let piece_ir = PieceIR {
            name: None,
            texture: "background".into(),
            transform: V::origin().into(),
            colliders: vec![PieceColliderIR {
                layer: String::from("default"),
                shape: CollisionShapeIR::Identifier(EntityIdentifier::Name(String::from("entry"))),
            }],
            draw_layer: 0,
            tags: Vec::new(),
        };
        match self.create_piece(piece_ir) {
            Ok(e) => {
                self.piece_textures.insert(e, texture);
                self.tag_manager
                    .add_tags(e, [entry.directory.to_string_lossy().to_string()].iter());
                e
            }
            Err(msg) => panic!("Failed to create piece: {msg}"),
        }
    }
}

/// an iterator over items to draw on a board with assigned layers
struct BoardDrawIter<
    'a,
    I: Iterator<Item = PieceEntity>,
    FN: Fn(PieceEntity) -> Option<O>,
    C: IColor,
    R: IRect,
    V: IVec2,
    S: IConvexShape + FromLuaTable + Debug + From<R>,
    T,
    F,
    O,
> {
    board: &'a BoardManager<C, R, V, S, T, F>,
    layered_entity_iter: I,
    pile_iter: Option<std::iter::Cloned<std::collections::vec_deque::Iter<'a, PieceEntity>>>,
    output_filter_map: FN,
}

impl<
    'a,
    I: Iterator<Item = PieceEntity>,
    FN: Fn(PieceEntity) -> Option<O>,
    C: IColor,
    R: IRect,
    V: IVec2,
    S: IConvexShape + FromLuaTable + Debug + From<R>,
    T,
    F,
    O,
> Iterator for BoardDrawIter<'a, I, FN, C, R, V, S, T, F, O>
{
    type Item = O;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let output_opt = if let Some(iter) = &mut self.pile_iter {
                if let Some(e) = iter.next() {
                    (self.output_filter_map)(e)
                } else {
                    self.pile_iter = None;
                    continue;
                }
            } else {
                let next_entity = self.layered_entity_iter.next()?;
                if self.board.pile_manager.is_pile(next_entity) {
                    self.pile_iter = Some(self.board.pile_manager.get_pieces_on_pile(next_entity));
                    continue;
                }

                (self.output_filter_map)(next_entity)
            };

            if let Some(o) = output_opt {
                return Some(o);
            }
        }
    }
}
