use crate::{
    board::{PieceEntity, players::PlayerColor},
    utils::ecs::{DenseComponentManager, DenseSet},
};

#[derive(Debug)]
pub struct SnapTargetManager {
    player_to_pile: DenseComponentManager<PlayerColor, PieceEntity>,
    pile_to_players: DenseComponentManager<PieceEntity, DenseSet<PlayerColor>>,
}

impl Default for SnapTargetManager {
    fn default() -> Self {
        Self {
            player_to_pile: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            pile_to_players: Default::default(),
        }
    }
}

impl SnapTargetManager {
    /// clears the target manager of all data
    pub fn clear(&mut self) {
        self.player_to_pile.clear();
        self.pile_to_players.clear();
    }

    /// sets the pile a player's contents is snapping to
    pub fn set_snap_target(&mut self, player: PlayerColor, pile_opt: Option<PieceEntity>) {
        // get rid of player's original snap target
        if let Some(old_pile) = self.player_to_pile.get(player) {
            let players = self.pile_to_players.get_mut(*old_pile).unwrap();
            players.remove(player);

            if players.is_empty() {
                self.pile_to_players.remove(*old_pile);
            }
        }

        // set new snap target
        if let Some(pile) = pile_opt {
            self.player_to_pile.insert(player, pile);
            self.pile_to_players
                .get_mut_or_insert(pile, || DenseSet::with_capacity(PlayerColor::COLOR_COUNT))
                .insert(player);
        } else {
            self.player_to_pile.remove(player);
        }
    }

    // removes a pile from the snap manager, returning the set of players that snap to the pile
    pub fn remove_snap_target(&mut self, pile: PieceEntity) -> DenseSet<PlayerColor> {
        if let Some(players) = self.pile_to_players.remove(pile) {
            for player in players.iter() {
                self.player_to_pile.remove(player);
            }
            players
        } else {
            DenseSet::with_capacity(0)
        }
    }

    /// gets the pile a player's contents is snapping to
    pub fn get_snap_target(&self, player: PlayerColor) -> Option<PieceEntity> {
        self.player_to_pile.get(player).cloned()
    }

    /// gets the set of players that snap to the given pile
    pub fn get_snapped_players(&self, pile: PieceEntity) -> Option<&DenseSet<PlayerColor>> {
        self.pile_to_players.get(pile)
    }
}
