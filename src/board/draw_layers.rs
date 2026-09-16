use std::collections::BTreeMap;

use crate::{board::PieceEntity, utils::ecs::DenseComponentManager};

#[derive(Debug)]
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
    /// clears the maanager of all draw layers
    pub fn clear(&mut self) {
        self.draw_layer_refs.clear();
        self.piece_draw_layers.clear();
    }

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

    /// the number of draw layers
    pub fn len(&self) -> usize {
        self.draw_layer_refs.len()
    }

    pub fn layer_iter(&self) -> impl Iterator<Item = i32> {
        self.draw_layer_refs.keys().copied()
    }
}

