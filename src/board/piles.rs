use std::{collections::VecDeque, iter::Cloned};

use crate::{board::PieceEntity, utils::{ecs::DenseComponentManager, geometry::IVec2}};

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
#[derive(Debug)]
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
    /// clears the pile manager of all date
    pub fn clear(&mut self) {
        self.piles.clear();
        self.piece_pile_map.clear();
    }

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

    /// gets snap info for the given pile (offset, radius)
    pub fn get_snap_info(&self, pile: PieceEntity) -> Option<(V, f32)> {
        self.piles.get(pile).map(|s| (s.snap_offset, s.snap_radius))
    }

    /// gets the snap offset of the pile
    pub fn get_snap_offset(&self, pile: PieceEntity) -> Option<V> {
        self.piles.get(pile).map(|s| s.snap_offset)
    }

    /// gets sets the snap offset of the pile
    pub fn set_snap_offset(&mut self, pile: PieceEntity, snap_offset: V) {
        if let Some(s) = self.piles.get_mut(pile) {
            s.snap_offset = snap_offset;
        }
    }

    /// gets the snap radius of the pile
    pub fn get_snap_radius(&self, pile: PieceEntity) -> Option<f32> {
        self.piles.get(pile).map(|s| s.snap_radius)
    }

    /// gets sets the snap radius of the pile
    pub fn set_snap_radius(&mut self, pile: PieceEntity, snap_radius: f32) {
        if let Some(s) = self.piles.get_mut(pile) {
            s.snap_radius = snap_radius;
        }
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
            !self.is_pile(entity),
            "Attempted to recreate an existing pile."
        );
        debug_assert!(
            !self.is_on_pile(entity),
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
    /// returns the pile that contained the piece
    pub fn unpile(&mut self, piece: PieceEntity) -> Option<PieceEntity> {
        if let Some(pile) = self.get_containing_pile(piece) {
            let pieces = &mut self.piles.get_mut(pile).unwrap().pieces;
            pieces.remove(pieces.iter().position(|p| *p == piece).unwrap());
            Some(pile)
        } else {
            None
        }
    }

    /// appends the contents of the the top pile onto the bottom pile
    /// panics if the piece pile being pushed is non-existent
    pub fn push_pile(&mut self, bottom_pile: PieceEntity, top_pile: PieceEntity) {
        debug_assert!(
            self.is_pile(bottom_pile),
            "Attempted to push to a non-existent pile."
        );
        debug_assert!(
            self.is_pile(top_pile),
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
            self.is_pile(pile),
            "Attempted to push to a non-existent pile."
        );

        self.piles.get_mut(pile).unwrap().pieces.push_back(piece);
        self.piece_pile_map.insert(piece, pile);
    }

    /// adds a piece to the bottom of the given pile
    /// panics if the piece being pushed already on a pile or if the pile doesnt exist
    pub fn push_bottom(&mut self, pile: PieceEntity, piece: PieceEntity) {
        debug_assert!(
            !self.is_on_pile(piece),
            "You may not push an already-piled piece onto different pile."
        );
        debug_assert!(
            self.is_pile(pile),
            "Attempted to push to a non-existent pile."
        );

        self.piles.get_mut(pile).unwrap().pieces.push_front(piece);
        self.piece_pile_map.insert(piece, pile);
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

