use std::{collections::VecDeque, path::Path};

use mlua::Lua;

use crate::{
    color::IColor,
    ecs::{DenseComponentManager, DenseSet, EntityTrait, NamedEntityManager, ResourceManager},
    geometry::{self, CollisionShape, IConvexShape, IRect, IVec2, Transform},
    intermediate_representations::{
        CollisionShapeIR, EntityIdentifier, PieceColliderIR, PieceIR, VisualInfo,
    },
    lua::FromLuaTable,
    renderer::{GlobalAssets, IRenderer},
    resources::{
        CategoryManager, CollisionLayerManager, CollisionShapeEntity, DrawLayerManager,
        PieceCollider, PieceEntity, PlayerColor, PlayerManager, QuadTreeManager, StackInfo,
        TextureEntity, TextureManager,
    },
};

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
    pub fn disable_collisions(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.no_collision_pieces.insert(piece);
        Ok(())
    }

    /// enable a piece's collisions
    pub fn enable_collisions(
        &mut self,
        piece: EntityIdentifier<PieceEntity>,
    ) -> Result<(), String> {
        let piece = self.is_piece_valid(piece)?;
        self.no_collision_pieces.remove(piece);
        Ok(())
    }
}
