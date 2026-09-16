use crate::{
    board::{PieceEntity, QuadTree}, utils::{
        ecs::{DenseComponentManager, Entity, ResourceManager},
        geometry::{IRect, IVec2},
    }
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionShapeEntityMarker;
pub type CollisionShapeEntity = Entity<CollisionShapeEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionLayerEntityMarker;
pub type CollisionLayerEntity = Entity<CollisionLayerEntityMarker>;

#[derive(Debug, Clone)]
pub struct PieceCollider {
    /// the collision shape entity
    pub shape: CollisionShapeEntity,

    /// the collision layers this collider "cares about"
    pub layer: CollisionLayerEntity,
}

#[derive(Debug)]
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
    /// clears the manager of all shapes
    pub fn clear(&mut self) {
        self.layers.clear();
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

#[derive(Debug)]
pub struct QuadTreeManager<R: IRect> {
    /// aabbs for each piece
    aabbs: DenseComponentManager<PieceEntity, R>,

    /// the quadtree used to track collisions
    quadtree: QuadTree<PieceEntity, R>,
}

impl<R: IRect> QuadTreeManager<R> {
    pub fn new(rect: R) -> Self {
        Self {
            aabbs: Default::default(),
            quadtree: QuadTree::new(rect),
        }
    }

    /// clears the quadtree of all data
    pub fn clear(&mut self) {
        self.aabbs.clear();
        self.quadtree.clear();
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

    /// returns an iterator over contained entities
    pub fn entity_iter(&self) -> impl Iterator<Item = PieceEntity> {
        self.aabbs.entity_iter()
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
