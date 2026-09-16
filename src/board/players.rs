use std::fmt::Display;

use mlua::Value;
use serde::Serialize;

use crate::{board::PieceEntity, lua::FromLuaValue, utils::{color::IColor, ecs::{DenseComponentManager, DenseSet, EntityTrait}, geometry::IVec2}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
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
    /// the number of colors
    pub const COLOR_COUNT: usize = 12;

    /// the list of colors
    pub const LIST: [Self; Self::COLOR_COUNT] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Lime,
        Self::Green,
        Self::Cyan,
        Self::Blue,
        Self::Purple,
        Self::Pink,
        Self::Black,
        Self::White,
        Self::Brown,
    ];

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

impl TryFrom<&str> for PlayerColor {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match Self::from_name(&value) {
            Some(s) => Ok(s),
            None => Err(format!("Unrecognized color: {value}")),
        }
    }
}

impl TryFrom<&String> for PlayerColor {
    type Error = String;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
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

#[derive(Debug, Clone)]
pub struct PlayerManager<V: IVec2> {
    /// player colors in use
    players: DenseSet<PlayerColor>,

    /// positions of player's hands
    positions: DenseComponentManager<PlayerColor, V>,

    /// the set of hidden players
    hidden: DenseSet<PlayerColor>,

    /// the piece being held by players
    held_items: DenseComponentManager<PlayerColor, PieceEntity>,

    /// the player holding a given piece
    item_holders: DenseComponentManager<PieceEntity, PlayerColor>,
}

impl<V: IVec2> Default for PlayerManager<V> {
    fn default() -> Self {
        Self {
            players: DenseSet::with_capacity(PlayerColor::COLOR_COUNT),
            positions: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            hidden: DenseSet::with_capacity(PlayerColor::COLOR_COUNT),
            held_items: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
            item_holders: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
        }
    }
}

impl<V: IVec2> PlayerManager<V> {
    /// returns an iterator over all held pieces
    pub fn held_item_iter(&self) -> impl Iterator<Item = PieceEntity> {
        self.held_items.component_iter().cloned()
    }

    /// returns the number of players
    pub fn len(&self) -> usize {
        self.players.len()
    }

    /// returns an iterator over players and their positions
    pub fn position_iter(&self) -> impl Iterator<Item = (PlayerColor, V)> {
        self.positions.pairs_iter().map(|(p, t)| (p, *t))
    }

    /// creates a new player with the given name and color
    pub fn new_player(&mut self, color: PlayerColor) -> Result<(), String> {
        if self.players.contains(color) {
            return Err(format!("Player color {color} is already taken."));
        }

        self.players.insert(color);
        self.positions.insert(color, V::from_xy(0.0, 0.0));
        Ok(())
    }

    /// hides the given player, or does nothing if the player is hidden or doesn't exist
    pub fn hide(&mut self, player: PlayerColor) {
        if self.players.contains(player) {
            self.hidden.insert(player);
        }
    }

    /// shows the given player, or does nothing if the player is visible or doesn't exist
    pub fn show(&mut self, player: PlayerColor) {
        self.hidden.remove(player);
    }

    /// checks if the given player is visible
    pub fn is_visible(&self, player: PlayerColor) -> bool {
        self.is_playing(player) && !self.hidden.contains(player)
    }

    /// checks if a player is contained
    pub fn is_playing(&self, color: PlayerColor) -> bool {
        self.players.contains(color)
    }

    /// removes a player, does nothing if the player does not exist
    pub fn remove_player(&mut self, player: PlayerColor) {
        self.players.remove(player);
        self.positions.remove(player);
        self.hidden.remove(player);
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

    /// checks if the player is holding a piece
    pub fn is_holding(&self, player: PlayerColor) -> bool {
        self.held_items.contains(player)
    }

    /// checks if a piece is being held by any of the player
    pub fn is_held(&self, piece: PieceEntity) -> bool {
        self.item_holders.contains(piece)
    }

    /// gets the player holding the given piece
    pub fn get_holder(&self, piece: PieceEntity) -> Option<PlayerColor> {
        self.item_holders.get(piece).copied()
    }

    /// gets the piece that the player is holding
    pub fn get_held_item(&self, player: PlayerColor) -> Option<PieceEntity> {
        self.held_items.get(player).copied()
    }

    /// sets the piece that the player is holding, returning the old held item
    /// panics if the target is already being held by another player
    pub fn set_held_item(
        &mut self,
        player: PlayerColor,
        target: Option<PieceEntity>,
    ) -> Option<PieceEntity> {
        if let Some(t) = target {
            assert!(!self.item_holders.contains(t));
            self.held_items.insert(player, t)
        } else {
            if let Some(piece) = self.held_items.remove(player) {
                self.item_holders.remove(piece);
                Some(piece)
            } else {
                None
            }
        }
    }

    /// returns an iterator over player colors in use
    pub fn iter(&self) -> impl Iterator<Item = PlayerColor> {
        self.players.iter()
    }

    /// resets all players to their original state: nothing held, all visible
    pub fn reset_player_states(&mut self) {
        let players: Vec<PlayerColor> = self.iter().collect();
        for player in players {
            self.set_held_item(player, None);
            self.show(player);
        }
    }
}

