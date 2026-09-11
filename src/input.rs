use std::{collections::HashSet, path::PathBuf};

use crate::{color::IColor, ecs::DenseComponentManager, geometry::IVec2, resources::PlayerColor};
use mlua::Lua;
use rand::{rngs::ThreadRng, seq::SliceRandom};
use serde::Deserialize;

/// layout schemes, defined by their "primary" element
/// TODO:
///  - item selection
///  - canvas
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutScheme<C: IColor> {
    /// no "main" element, just use supplied "basic" elements
    NoPrimary(ModularLayout<C>),

    /// trackpad is primary element
    Trackpad(ModularLayout<C>),
}

/// modular input layout, created by rows of input elements
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModularLayout<C: IColor> {
    layout: Vec<Vec<InputElement<C>>>,
}

impl<C: IColor> ModularLayout<C> {
    pub fn new(layout: Vec<Vec<InputElement<C>>>) -> Result<Self, String> {
        // ensure all input element names are unique
        let mut name_set = HashSet::new();
        for row in layout.iter() {
            for element in row {
                let name: &str = &element.name;
                if !name_set.insert(name) {
                    return Err(format!("Layout has duplicate input name: {name}."));
                }
            }
        }

        Ok(Self { layout })
    }

    /// gets a reference to the underlying layout
    pub fn layout(&self) -> &Vec<Vec<InputElement<C>>> {
        &self.layout
    }

    /// gets the element with the given name
    pub fn get_element(&self, name: &str) -> Option<&InputElement<C>> {
        self.layout
            .iter()
            .flat_map(|row| row.iter())
            .find(|element| element.name == name)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct DeserializableInputEvent {
    /// the name of the input being interacted with
    pub name: Option<String>,

    /// the kind of input being interacted with
    pub kind: String,

    /// for trackpad movement, change in x
    pub delta_x: Option<f32>,

    /// for trackpad movement, change in y
    pub delta_y: Option<f32>,
}

/// an input event associated with a player
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInputEvent {
    pub player: PlayerColor,
    pub event: InputEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    ButtonPress { name: String },
    TrackpadDelta { delta_x: f32, delta_y: f32 },
    TrackpadPress,
}

impl TryFrom<&DeserializableInputEvent> for InputEvent {
    type Error = String;

    fn try_from(value: &DeserializableInputEvent) -> Result<Self, Self::Error> {
        let kind_str: &str = &value.kind;
        match kind_str {
            "ButtonPress" => {
                if let Some(name) = value.name.clone() {
                    Ok(Self::ButtonPress { name })
                } else {
                    return Err(format!("ButtonPress event is missing a name."));
                }
            }

            "TrackpadPress" => Ok(Self::TrackpadPress),

            "TrackpadDelta" => {
                let delta_x = value.delta_x.unwrap_or(0.0);
                let delta_y = value.delta_y.unwrap_or(0.0);

                if !delta_x.is_finite() {
                    return Err(format!("Invalid delta_x: non-finite value."));
                }
                if !delta_y.is_finite() {
                    return Err(format!("Invalid delta_y: non-finite value."));
                }

                Ok(Self::TrackpadDelta { delta_x, delta_y })
            }

            _ => Err(format!("Invalid input event kind '{}'", &value.kind)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputElement<C: IColor> {
    /// the name of the element
    pub name: String,

    /// the descriptors of the element
    pub info: InputElementInfo<C>,
}

/// modules in player input devices
/// TODO:
///  - slider
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputElementInfo<C: IColor> {
    /// button activates once for the frame it was pressed
    Button(ButtonInfo<C>),
}

/// icon or string to display on an input element
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    Name(String),
    Icon(PathBuf),
}

/// info for a button
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonInfo<C: IColor> {
    label: Label,
    label_color: C,
    background_color: C,
}

/// player input state
#[derive(Debug)]
pub struct PlayerInputState {
    pub trackpad_delta: (f32, f32),
    pub trackpad_pressed: bool,
    pub pressed_buttons: Vec<String>,
}

impl Default for PlayerInputState {
    fn default() -> Self {
        Self {
            trackpad_delta: <(f32, f32)>::origin(),
            trackpad_pressed: false,
            pressed_buttons: Vec::new(),
        }
    }
}

impl PlayerInputState {
    fn clear(&mut self) {
        self.trackpad_delta = <(f32, f32)>::origin();
        self.trackpad_pressed = false;
        self.pressed_buttons.clear();
    }

    pub fn handle_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::ButtonPress { name } => {
                if !self.pressed_buttons.contains(&name) {
                    self.pressed_buttons.push(name);
                }
            }

            InputEvent::TrackpadPress => {
                self.trackpad_pressed = true;
            }

            InputEvent::TrackpadDelta { delta_x, delta_y } => {
                self.trackpad_delta = self.trackpad_delta.add((delta_x, delta_y));
            }
        }
    }

    pub fn extract(&mut self, lua: &Lua, player_name: &str) -> Option<mlua::Table> {
        if self.trackpad_delta == <(f32, f32)>::origin()
            && !self.trackpad_pressed
            && self.pressed_buttons.is_empty()
        {
            return None;
        }

        let base_table = lua
            .create_table_with_capacity(0, 4)
            .expect("Unable to create player input table");

        let delta = match self.trackpad_delta.into_lua_table(lua) {
            Ok(d) => d,
            Err(msg) => panic!("Unable to create player input table: {msg}"),
        };

        let buttons_pressed = lua
            .create_table_with_capacity(0, self.pressed_buttons.len())
            .expect("Unable to create player input table");
        for button in &self.pressed_buttons {
            buttons_pressed
                .raw_set(button.as_str(), true)
                .expect("Unable to create player input table");
        }

        base_table
            .raw_set("player", player_name)
            .expect("Unable to create player input table");
        base_table
            .raw_set("trackpad_delta", delta)
            .expect("Unable to create player input table");
        base_table
            .raw_set("trackpad_pressed", self.trackpad_pressed)
            .expect("Unable to create player input table");
        base_table
            .raw_set("button_presses", buttons_pressed)
            .expect("Unable to create player input table");

        self.clear();

        Some(base_table)
    }
}

/// global input state
#[derive(Debug)]
pub struct InputState {
    shuffler: ThreadRng,
    player_map: DenseComponentManager<PlayerColor, Box<PlayerInputState>>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            shuffler: ThreadRng::default(),
            player_map: DenseComponentManager::with_capacity(PlayerColor::COLOR_COUNT),
        }
    }
}

impl InputState {
    /// handles a single input event
    fn handle_event(&mut self, player_event: PlayerInputEvent) {
        if let Some(state) = self.player_map.get_mut(player_event.player) {
            state.handle_event(player_event.event);
        }
    }

    /// adds a player
    pub fn add_player(&mut self, player: PlayerColor) {
        if !self.player_map.contains(player) {
            self.player_map.insert(player, Default::default());
        }
    }

    /// removes a player
    pub fn remove_player(&mut self, player: PlayerColor) {
        self.player_map.remove(player);
    }

    /// updates input state for the given frame, returning a lua table listing input events
    pub fn update(
        &mut self,
        lua: &Lua,
        event_iter: impl Iterator<Item = PlayerInputEvent>,
    ) -> mlua::Table {
        for event in event_iter {
            self.handle_event(event);
        }

        let mut players: Vec<PlayerColor> = self.player_map.entity_iter().collect();
        players.shuffle(&mut self.shuffler);

        let output_table = lua
            .create_table_with_capacity(players.len(), 0)
            .expect("Unable to create player input table");
        for (idx, player) in players.into_iter().enumerate() {
            if let Some(input_table) = self
                .player_map
                .get_mut(player)
                .unwrap()
                .extract(lua, player.name())
            {
                output_table
                    .raw_set(idx + 1, input_table)
                    .expect("Unable to create player input table");
            }
        }

        output_table
    }
}
