use std::collections::{BTreeMap, VecDeque};

use crate::{
    ecs::{DenseComponentManager, DenseSet, Entity, NamedEntityManager},
    geometry::{IRect, IVec2, Transform},
    quadtree::QuadTree,
    resources::{CollisionLayerEntity, DrawLayerEntity, TextureEntity},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceEntityMarker;
pub type PieceEntity = Entity<PieceEntityMarker>;

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
}

pub struct BoardManager<R: IRect, V: IVec2> {
    /// the entities for each piece
    piece_entities: NamedEntityManager<PieceEntity>,

    /// stacks of pieces
    stacks: DenseComponentManager<PieceEntity, StackInfo<V>>,

    /// the drawing layers of each piece
    draw_layer: DenseComponentManager<PieceEntity, DrawLayerEntity>,

    /// the order in which to draw entities,
    /// adding/removing a texture adds/removes an entity from this list
    /// picking up an entity places it on the top of this list
    draw_order: VecDeque<PieceEntity>,

    /// the textures for each piece
    textures: DenseComponentManager<PieceEntity, TextureEntity>,

    /// the transforms of each piece
    transforms: DenseComponentManager<PieceEntity, Transform<V>>,

    /// the colliders for each piece
    colliders: DenseComponentManager<PieceEntity, Vec<PieceCollider>>,

    /// neat aabbs for each piece
    /// aabbs may be removed when pieces get picked up
    aabbs: DenseComponentManager<PieceEntity, R>,

    /// the quadtree used to track collisions
    quadtree: QuadTree<PieceEntity, R>,

    /// the categorizations of each piece
    categories: BTreeMap<String, DenseSet<PieceEntity>>,
}
