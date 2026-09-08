use std::{collections::VecDeque, path::Path};

use mlua::Lua;

use crate::{
    color::IColor,
    ecs::{
        DenseComponentManager, DenseSet, EntityTrait, NamedEntityManager, ResourceManager,
        SparseSet,
    },
    geometry::{self, CollisionShape, IConvexShape, IRect, IVec2, RigidTransform},
    intermediate_representations::{
        CollisionShapeIR, EntityIdentifier, PieceColliderIR, PieceIR, PileIR, VisualInfo,
    },
    lua::FromLuaTable,
    renderer::{GlobalAssets, ITextureCompiler},
    resources::{
        CategoryManager, CollisionLayerEntity, CollisionLayerManager, CollisionShapeEntity,
        DrawLayerManager, PieceCollider, PieceEntity, PileManager, PlayerColor, PlayerManager,
        QuadTreeManager, TextureEntity, TextureManager,
    },
};

/// Manager for pieces on a board
/// Each PieceEntity falls under one of three mutually exclusive sets
///  - piles
///  - unpiled
///  - piled
pub struct BoardManager<R: IRect, V: IVec2, S: IConvexShape + FromLuaTable, T, F> {
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

    /// the categories of entities
    category_manager: CategoryManager,

    /// the entity manager for pieces and piles
    piece_entity_manager: NamedEntityManager<PieceEntity>,

    /// piles of pieces
    pile_manager: PileManager<V>,

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
    debug_pieces: DenseSet<PieceEntity>,

    /// entities who should be hidden
    hidden_entities: DenseSet<PieceEntity>,

    /// pieces who have collisions disabled
    no_collision_entities: DenseSet<PieceEntity>,

    /// the order in which to draw entities
    /// RULES:
    ///  - NO PILED PIECES may be stored here
    entity_draw_order: VecDeque<PieceEntity>,

    /// the transforms of each piece
    /// piles and un-piled pieces use absolute transforms
    /// piled pieces have transforms relative to their parent pile
    /// RULES:
    ///  - ALL ENTITIES must have a transform
    ///  - Transforms may not place an entity's center or colliders outside of quadtree bounds
    entity_transforms: DenseComponentManager<PieceEntity, RigidTransform<V>>,

    /// the colliders for each piece
    /// we must ensure that each piece collider vector is non-emtpy
    /// RULES:
    ///  - NO PILES may have colliders
    ///  - Transformed colliders may NOT exist outside of quadtree bounds
    piece_colliders: DenseComponentManager<PieceEntity, Vec<PieceCollider>>,

    /// quadtree manager that tracks piece aabb collisions
    /// also tracks the board's dimensions
    /// RULES:
    ///  - NO PILES may be contained
    ///  - HELD ENTITIES may NOT be contained
    quadtree: QuadTreeManager<R>,
}

impl<R: IRect, V: IVec2, S: IConvexShape + FromLuaTable, T, F> BoardManager<R, V, S, T, F> {
    pub fn new(dimensions: V) -> Self {
        Self {
            texture_manager: Default::default(),
            player_manager: Default::default(),
            collision_shape_manager: Default::default(),
            collision_layer_manager: Default::default(),
            entity_draw_layers: Default::default(),
            category_manager: Default::default(),
            piece_entity_manager: Default::default(),
            debug_pieces: Default::default(),
            hidden_entities: Default::default(),
            no_collision_entities: Default::default(),
            pile_manager: Default::default(),
            piece_textures: Default::default(),
            entity_draw_order: Default::default(),
            entity_transforms: Default::default(),
            piece_colliders: Default::default(),
            quadtree: QuadTreeManager::new(dimensions),
        }
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
        offset: V,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;

        let angle = self.entity_transforms.get(entity).unwrap().angle;

        let transform = RigidTransform { offset, angle };

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

        let offset = self.entity_transforms.get(entity).unwrap().offset;

        let transform = RigidTransform { offset, angle };

        self.set_transform_unchecked(entity, transform);
        Ok(())
    }

    /// centers the item on the board
    pub fn center_item(
        &mut self,
        entity: EntityIdentifier<PieceEntity>,
    ) -> Result<(), String> {
        let entity = self.validate_entity(entity)?;
        self.center_entity_unchekced(entity);
        Ok(())
    }

    /// centers the item on the board
    /// does not check if the entity is valid
    fn center_entity_unchekced(&mut self, entity: PieceEntity) {
        let global_transform = self.get_entity_global_transform(entity);
        let center = self.dimensions().scaled(0.5);
        let delta = center.sub(global_transform.offset);

        if let Some(parent) = self.pile_manager.get_containing_pile(entity) {
            let parent_transform = self.entity_transforms.get(parent).unwrap();
            self.set_pile_transform_unchecked(
                parent,
                RigidTransform {
                    offset: parent_transform.offset.add(delta),
                    angle: parent_transform.angle,
                },
            );
        } else {
            let original_transform = self.entity_transforms.get(entity).unwrap();
            self.set_transform_unchecked(
                entity,
                RigidTransform {
                    offset: original_transform.offset.add(delta),
                    angle: original_transform.angle,
                },
            );
        }
    }

    /// sets the transform of a pile or piece without checking whether the entity is valid
    fn set_transform_unchecked(&mut self, entity: PieceEntity, transform: RigidTransform<V>) {
        if self.pile_manager.is_pile(entity) {
            self.set_pile_transform_unchecked(entity, transform);
        } else {
            self.set_piece_transform_unchecked(entity, transform);
        }
    }

    /// sets the transform of a pile without checking whether the pile is valid
    /// the pile and the contained pieces must have their origin in the board
    /// colliders must also fit entirely in the board
    fn set_pile_transform_unchecked(&mut self, pile: PieceEntity, transform: RigidTransform<V>) {
        // used to move pieces after moving pile
        let mut piece_aabbs = DenseComponentManager::new();

        // compute initial aabb
        let initial_aabb = {
            let mut rect = R::empty(transform.offset);

            for piece in self.pile_manager.get_pieces_on_pile(pile) {
                let piece_transform =
                    transform.apply_to_other(self.entity_transforms.get(piece).unwrap());
                let collider_opt = self.piece_colliders.get(piece);
                let piece_aabb = if let Some(colliders) = collider_opt {
                    let aabb = geometry::compute_advanced_aabb::<R, V, S>(
                        &piece_transform,
                        colliders
                            .iter()
                            .map(|c| self.collision_shape_manager.get(c.shape).unwrap()),
                    )
                    .unwrap()
                    .bounding(&R::empty(piece_transform.offset));
                    piece_aabbs.insert(piece, aabb);
                    aabb
                } else {
                    R::empty(piece_transform.offset)
                };

                rect = rect.bounding(&piece_aabb);
            }

            rect
        };

        // determine final transform
        let final_aabb: R = self.quadtree.rect().fit(initial_aabb);
        let delta = final_aabb.min::<V>().sub(initial_aabb.min::<V>());
        let transform = RigidTransform {
            offset: transform.offset.add(delta),
            angle: transform.angle,
        };

        self.entity_transforms.insert(pile, transform);

        // move piece aabbs
        if !self.is_entity_held(pile) {
            for (piece, aabb) in piece_aabbs.pairs_iter() {
                self.quadtree.set_aabb(piece, aabb.offset(delta));
            }
        }
    }

    /// sets the transform of a piece without checking whether the piece is valid
    /// must ensure that
    /// - the piece's origin is fully contained in our board
    /// - if the piece has colliders, they are fully contained in our board
    fn set_piece_transform_unchecked(&mut self, piece: PieceEntity, transform: RigidTransform<V>) {
        let parent_transform = self.get_parent_transform(piece);

        if let Some(colliders) = self.piece_colliders.get(piece) {
            // if this piece has colliders...
            // ensure they are fully contained by the board

            // compute the global aabb
            let global_transform = parent_transform.apply_to_other(&transform);
            let initial_aabb = geometry::compute_advanced_aabb::<R, V, S>(
                &global_transform,
                colliders
                    .iter()
                    .map(|c| self.collision_shape_manager.get(c.shape).unwrap()),
            )
            .unwrap()
            .bounding(&R::empty(global_transform.offset));

            // the board-fitted global aabb
            let final_aabb: R = self.quadtree.rect().fit(initial_aabb);

            let global_delta = final_aabb.min::<V>().sub(initial_aabb.min::<V>());
            let relative_delta = parent_transform.apply_inv_to_point(global_delta);

            let transform = RigidTransform {
                offset: transform.offset.add(relative_delta),
                angle: transform.angle,
            };

            self.entity_transforms.insert(piece, transform);

            // update quadtree only if player is not held
            // (setting aabb may insert it)
            if !self.is_entity_held(piece) {
                self.quadtree.set_aabb(piece, final_aabb);
            }
        } else {
            // clamp transform so that its offset fits in the board
            let initial_global_transform = parent_transform.apply_to_other(&transform);
            let final_global_transform = RigidTransform {
                offset: self
                    .quadtree
                    .rect()
                    .fit_point(initial_global_transform.offset),
                angle: transform.angle,
            };
            let global_delta = final_global_transform
                .offset
                .sub(initial_global_transform.offset);
            let relative_delta = parent_transform.apply_inv_to_point(global_delta);

            let transform = RigidTransform {
                offset: transform.offset.add(relative_delta),
                angle: transform.angle,
            };

            self.entity_transforms.insert(piece, transform);
        }
    }

    /// sets the categories of a piece
    pub fn set_categories(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        categories: &[String],
    ) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;

        self.category_manager.set_categories(piece, categories);

        Ok(())
    }

    /// adds categories to a piece
    pub fn add_categories(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        categories: &[String],
    ) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;

        self.category_manager.add_categories(piece, categories);

        Ok(())
    }

    /// removes categories from a piece
    pub fn remove_categories(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
        categories: &[String],
    ) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;

        self.category_manager.remove_categories(piece, categories);
        Ok(())
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

        self.category_manager.add_categories(entity, &ir.categories);

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

        self.entity_draw_layers.set(entity, ir.draw_layer);

        self.entity_draw_order.push_back(entity);

        self.category_manager.add_categories(entity, &ir.categories);

        self.set_pile_transform_unchecked(entity, ir.transform);

        Ok(entity)
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
                let transform = self.entity_transforms.get(piece).unwrap();
                let draw_layer = self.entity_draw_layers.get(piece).unwrap();
                let colliders = if self.debug_pieces.contains(piece) {
                    self.piece_colliders.get(piece).map(|l| l.as_slice())
                } else {
                    None
                };

                Some((texture, colliders, *transform, draw_layer))
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
                let transform = self.entity_transforms.get(piece).unwrap();
                let colliders = if self.debug_pieces.contains(piece) {
                    self.piece_colliders.get(piece).map(|l| l.as_slice())
                } else {
                    None
                };

                Some((texture, colliders, *transform))
            },
        }
    }

    /// returns an iterator over players, their positions, and their held pieces
    /// players should be drawn on top of every piece
    pub fn player_draw_iter<'a, 'b, C: IColor>(
        &'b self,
        global_assets: &'a GlobalAssets<T>,
    ) -> impl Iterator<Item = (&'a T, V, C)> {
        self.player_manager.position_iter().map(|(color, pos)| {
            let texture = if self.player_manager.is_holding(color) {
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
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        self.texture_manager.load_texture(
            project_path,
            Some(name.clone()),
            visual,
            magnification_dimensions,
            renderer,
        )?;

        Ok(())
    }

    pub fn add_player(&mut self, color: PlayerColor, name: String) -> Result<(), String> {
        self.player_manager.new_player(color, name)
    }

    pub fn remove_player(&mut self, color: PlayerColor) {
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
    pub fn query_pile_overlaps(
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

    pub fn query_piece_overlaps(
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

    /// returns the piece a player should grab, should it attempt to
    pub fn piece_to_grab(&mut self, player: PlayerColor) -> Option<PieceEntity> {
        let position = self.player_manager.get_position(player)?;

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

        unreachable!()
    }

    /// makes a piece show its collision shapes
    pub fn show_debug(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;
        self.debug_pieces.insert(piece);
        Ok(())
    }

    /// makes a piece hide its collision shapes
    pub fn hide_debug(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;
        self.debug_pieces.remove(piece);
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
            Some(pile) => *self
                .entity_transforms
                .get(pile)
                .unwrap_or(&RigidTransform::identity()),
            None => RigidTransform::identity(),
        }
    }

    /// gets the global transform of the given entity
    /// does not check if the entity is valid
    fn get_entity_global_transform(&self, entity: PieceEntity) -> RigidTransform<V> {
        self.get_parent_transform(entity)
            .apply_to_other(self.entity_transforms.get(entity).unwrap())
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

        // grab the item
        let _ = self.drop_held_item(player);
        if let Some(entity) = entity {
            let entity = self.validate_entity(entity)?;

            if self.pile_manager.is_pile(entity) {
                self.grab_pile(player, entity);
            } else {
                self.grab_piece(player, entity);
            }
        }

        // move player to valid position
        self.set_player_position(player, self.player_manager.get_position(player).unwrap())
            .unwrap();

        Ok(())
    }

    /// makes the player grab a pile
    /// assumes that
    ///  - the player is valid
    ///  - the pile is valid
    ///  - the player is not holding anything
    fn grab_pile(&mut self, player: PlayerColor, pile: PieceEntity) {
        if let Some(other_player) = self.player_manager.get_holder(pile) {
            // remove existing control of the pile
            self.player_manager.set_held_item(other_player, None);
        } else {
            // if pile is not yet held, make sure to grab the pieces
            let mut piece_set = DenseSet::new();
            for piece in self.pile_manager.get_pieces_on_pile(pile) {
                self.quadtree.remove(piece);
                piece_set.insert(piece);
            }
            self.entity_draw_order.retain(|p| !piece_set.contains(*p));
        }

        // now grab the pile
        self.player_manager.set_held_item(player, Some(pile));
    }

    /// makes the player grab a pile
    /// assumes that
    ///  - the player is valid
    ///  - the piece is valid
    ///  - the player is not holding anything
    fn grab_piece(&mut self, player: PlayerColor, piece: PieceEntity) {
        let is_held = self.is_entity_held(piece);

        // remove piece from its pile, if its a part of one
        // return piece to draw order as putting a piece on a pile removes
        // it from the draw order
        if let Some(pile) = self.pile_manager.unpile(piece) {
            let pile_idx = self
                .entity_draw_order
                .iter()
                .position(|p| *p == pile)
                .unwrap();
            self.entity_draw_order.insert(pile_idx + 1, piece);
        }

        // update the piece to a grabbed state
        if !is_held {
            self.quadtree.remove(piece);
        }
        self.player_manager.set_held_item(player, Some(piece));
    }

    /// makes the player drop is held piece or pile
    /// fails if the player doesnt exist
    pub fn drop_held_item(&mut self, player: PlayerColor) -> Result<(), String> {
        self.validate_player(player)?;

        if let Some(entity) = self.player_manager.get_held_item(player) {
            self.player_manager.set_held_item(player, None);

            // re-enable collisions by performing an operation that updates the quadtree
            self.set_transform_unchecked(entity, *self.entity_transforms.get(entity).unwrap());

            // put new piece at top of its draw layer
            self.entity_draw_order.remove(
                self.entity_draw_order
                    .iter()
                    .position(|e| *e == entity)
                    .unwrap(),
            );
            self.entity_draw_order.push_back(entity);
        }

        Ok(())
    }

    /// sets the position of the player
    /// fails if the player does not exist
    pub fn set_player_position(&mut self, player: PlayerColor, position: V) -> Result<(), String> {
        self.validate_player(player)?;

        // clamp position to board boundaries
        let position = self.quadtree.rect().fit_point(position);

        if let Some(piece) = self.player_manager.get_held_item(player) {
            // update held piece position
            // use updated position to set player position (preserves clamping rules)
            let _ = self.set_position(EntityIdentifier::Entity(piece), position);
            self.player_manager
                .set_position(player, self.get_entity_global_transform(piece).offset);
        } else {
            // update player position
            self.player_manager.set_position(player, position);
        }

        Ok(())
    }

    /// makes a piece visible
    pub fn show(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;
        self.hidden_entities.remove(piece);
        Ok(())
    }

    /// makes a piece hidden
    pub fn hide(&mut self, piece: EntityIdentifier<PieceEntity>) -> Result<(), String> {
        let piece = self.validate_piece(piece)?;
        self.hidden_entities.insert(piece);
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
}

/// an iterator over items to draw on a board with assigned layers
struct BoardDrawIter<
    'a,
    I: Iterator<Item = PieceEntity>,
    FN: Fn(PieceEntity) -> Option<O>,
    R: IRect,
    V: IVec2,
    S: IConvexShape + FromLuaTable,
    T,
    F,
    O,
> {
    board: &'a BoardManager<R, V, S, T, F>,
    layered_entity_iter: I,
    pile_iter: Option<std::iter::Cloned<std::collections::vec_deque::Iter<'a, PieceEntity>>>,
    output_filter_map: FN,
}

impl<
    'a,
    I: Iterator<Item = PieceEntity>,
    FN: Fn(PieceEntity) -> Option<O>,
    R: IRect,
    V: IVec2,
    S: IConvexShape + FromLuaTable,
    T,
    F,
    O,
> Iterator for BoardDrawIter<'a, I, FN, R, V, S, T, F, O>
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
