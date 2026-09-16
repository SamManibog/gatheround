use crate::{board::PieceEntity, utils::ecs::{DenseComponentManager, DenseSet, Entity, ResourceManager}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceTagEntityMarker;
pub type TagEntity = Entity<PieceTagEntityMarker>;

#[derive(Debug)]
pub struct TagManager {
    /// the tags of pieces that may exist
    /// maps tags to the list of pieces in that tag
    /// we must ensure that all tags are named entities
    tag_collections: ResourceManager<TagEntity, DenseSet<PieceEntity>>,

    /// the tags assigned to each piece
    piece_tags: DenseComponentManager<PieceEntity, DenseSet<TagEntity>>,
}

impl Default for TagManager {
    fn default() -> Self {
        Self {
            tag_collections: Default::default(),
            piece_tags: Default::default(),
        }
    }
}

impl TagManager {
    /// clears the tag manager of all tags
    pub fn clear(&mut self) {
        self.tag_collections.clear();
        self.piece_tags.clear();
    }

    /// adds tags to a piece
    pub fn add_tags<'a, 'b: 'a>(
        &'b mut self,
        piece: PieceEntity,
        tags: impl Iterator<Item = &'a String>,
    ) {
        let piece_tags = self.piece_tags.get_mut_or_insert(piece, || DenseSet::new());

        for tag in tags {
            let (tag_entity, tag_collection) =
                if let Some(e) = self.tag_collections.get_entity(&tag) {
                    (e, self.tag_collections.get_mut(e).unwrap())
                } else {
                    let entity = self
                        .tag_collections
                        .add_named(tag.clone(), DenseSet::new())
                        .unwrap();
                    (entity, self.tag_collections.get_mut(entity).unwrap())
                };

            piece_tags.insert(tag_entity);
            tag_collection.insert(piece);
        }
    }

    /// checks if a piece has the given tag
    pub fn has_tag(&self, piece: PieceEntity, tag: &String) -> bool {
        let tag_entity = if let Some(e) = self.tag_collections.get_entity(tag) {
            e
        } else {
            return false;
        };

        let piece_tags = if let Some(c) = self.piece_tags.get(piece) {
            c
        } else {
            return false;
        };

        piece_tags.contains(tag_entity)
    }

    /// sets the tags of a piece
    pub fn set_tags<'a, 'b: 'a>(
        &'b mut self,
        piece: PieceEntity,
        tags: impl Iterator<Item = &'a String>,
    ) {
        self.clear_tags(piece);
        self.add_tags(piece, tags);
    }

    /// clears all tags from a piece
    pub fn clear_tags(&mut self, piece: PieceEntity) {
        let piece_tags = if let Some(val) = self.piece_tags.remove(piece) {
            val
        } else {
            return;
        };

        for tag_entity in piece_tags.iter() {
            let tag_collection = self.tag_collections.get_mut(tag_entity).unwrap();
            tag_collection.remove(piece);
            if tag_collection.is_empty() {
                self.tag_collections.remove(tag_entity);
            }
        }

        self.piece_tags.remove(piece);
    }

    /// removes specific tags from a piece
    pub fn remove_tags<'a, 'b: 'a>(
        &'b mut self,
        piece: PieceEntity,
        tags: impl Iterator<Item = &'a String>,
    ) {
        let piece_tags = if let Some(val) = self.piece_tags.get_mut(piece) {
            val
        } else {
            return;
        };

        for tag in tags {
            if let Some(tag_entity) = self.tag_collections.get_entity(&tag) {
                let tag_collection = self.tag_collections.get_mut(tag_entity).unwrap();

                piece_tags.remove(tag_entity);
                tag_collection.remove(piece);

                if tag_collection.is_empty() {
                    self.tag_collections.remove(tag_entity);
                }
            }
        }

        if piece_tags.is_empty() {
            self.piece_tags.remove(piece);
        }
    }

    /// returns an iterator over all existing tags
    pub fn tags(&self) -> impl Iterator<Item = &String> {
        self.tag_collections
            .identifier_iter()
            .map(|(_, n)| n.unwrap())
    }

    /// returns an iterator over all tags assigned to a piece
    pub fn piece_tags(&self, piece: PieceEntity) -> impl Iterator<Item = &String> {
        self.piece_tags
            .get(piece)
            .into_iter()
            .flat_map(|c| c.iter().map(|e| self.tag_collections.get_name(e).unwrap()))
    }

    /// returns an iterator over all pieces in a tag
    pub fn tag_pieces(&self, tag: &String) -> impl Iterator<Item = PieceEntity> {
        self.tag_collections
            .get_by_name(tag)
            .into_iter()
            .flat_map(|c| c.iter())
    }
}

