use std::{
    cell::RefCell,
    fmt::Debug,
    path::{Path, PathBuf},
    rc::Rc,
};

use mlua::{Lua, LuaOptions, StdLib, Table, Value};

use crate::{
    board::{
        BoardManager,
        irs::{EntityIdentifier, PieceIR, PileIR, TextureIR},
        players::PlayerColor,
    },
    platform::IPlatform,
    utils::color::{IColor, color_from_lua_value},
    utils::ecs::EntityTrait,
    utils::geometry::{
        IConvexShape, IRect, IVec2, RigidTransform, finite_vec2_from_lua_table,
        finite_vec2_from_lua_value,
    },
};

/// for structs that can be created based off of a lua table
pub trait FromLuaTable: Sized {
    fn try_from_lua_table(table: &Table) -> Result<Self, String>;
}

/// for structs that can be created based off of a lua value
pub trait FromLuaValue: Sized {
    fn try_from_lua_value(value: &Value) -> Result<Self, String>;
}

pub fn finite_f32_from_lua_value(value: &Value) -> Result<f32, String> {
    match value {
        Value::Integer(number) => {
            let min = (f32::MIN as f64).floor() as i64;
            let max = (f32::MAX as f64).ceil() as i64;

            if *number < min || *number > max {
                return Err(format!("non-finite value."));
            } else {
                Ok(*number as f32)
            }
        }

        Value::Number(number) => {
            let min = f32::MIN as f64;
            let max = f32::MAX as f64;

            if !number.is_finite() || *number < min || *number > max {
                return Err(format!("non-finite value."));
            } else {
                Ok(*number as f32)
            }
        }

        _ => Err(format!("non-numeric type.")),
    }
}

impl FromLuaValue for String {
    fn try_from_lua_value(value: &Value) -> Result<Self, String> {
        if let Value::String(string) = value {
            if let Ok(s) = string.to_str() {
                Ok(s.to_string())
            } else {
                Err(format!("non-UTF-8 string."))
            }
        } else {
            Err(format!("non-string value."))
        }
    }
}

impl FromLuaValue for f32 {
    fn try_from_lua_value(value: &Value) -> Result<Self, String> {
        let value = match value {
            Value::Integer(n) => *n as f64,
            Value::Number(n) => *n as f64,
            _ => return Err(format!("non-numeric type.")),
        };

        Ok(value as f32)
    }
}

impl FromLuaValue for f64 {
    fn try_from_lua_value(value: &Value) -> Result<Self, String> {
        let value = match value {
            Value::Integer(n) => *n as f64,
            Value::Number(n) => *n as f64,
            _ => return Err(format!("non-numeric type.")),
        };

        Ok(value)
    }
}

/// gets a vector of values that can be constructed from lua values from a lua table
/// accepts a table or a nil value
pub fn vector_from_lua_value<T: FromLuaValue>(
    value: &Value,
    vector_name: &str,
    value_name: &str,
) -> Result<Vec<T>, String> {
    let mut values = Vec::new();

    if let Value::Nil = value {
        return Ok(Vec::new());
    }

    let values_table = if let Value::Table(t) = value {
        t
    } else {
        return Err(format!("Invalid {vector_name}: non-table value."));
    };

    for (index, result) in values_table.sequence_values::<Value>().enumerate() {
        if let Ok(value) = result {
            match T::try_from_lua_value(&value) {
                Ok(v) => values.push(v),
                Err(msg) => {
                    return Err(format!(
                        "Invalid {value_name} at index {}: {msg}",
                        index + 1
                    ));
                }
            }
        } else {
            return Err(format!("Invalid {value_name} at index {}.", index + 1));
        }
    }

    Ok(values)
}

#[inline]
fn create_game_api_function<F, A, B>(lua: &Lua, name: &str, func: F)
where
    F: Fn(&Lua, A) -> mlua::Result<B> + mlua::MaybeSend + 'static,
    A: mlua::FromLuaMulti,
    B: mlua::IntoLuaMulti,
{
    let function = lua
        .create_function_mut(func)
        .expect(&format!("Unable to create global function '{name}'"));
    lua.globals()
        .raw_set(name, function)
        .expect(&format!("Unable to create global function '{name}'"));
}

/// gets a lua instance with the proper game api
#[allow(unused_variables)]
pub fn get_lua_instance<
    C: IColor + 'static,
    R: IRect + 'static,
    V: IVec2 + 'static,
    S: IConvexShape + FromLuaTable + Debug + From<R> + 'static,
    T: 'static,
    F: 'static,
    P: IPlatform<Texture = T, Font = F, Color = C, Rectangle = R, Vector2 = V, ConvexShape = S>,
>(
    project_path: &Path,
    board_cell: &Rc<RefCell<BoardManager<C, R, V, S, T, F>>>,
    platform_cell: &Rc<RefCell<P>>,
) -> Lua {
    let exclude_libs = StdLib::IO | StdLib::OS | StdLib::COROUTINE;
    let libs = StdLib::ALL_SAFE & !exclude_libs;
    let lua = Lua::new_with(libs, LuaOptions::default()).expect("Unable to create lua instance.");

    let board_cell_clone = board_cell.clone();
    let platform_cell_clone = platform_cell.clone();

    let project_path = PathBuf::from(project_path);
    let board_cell = board_cell_clone.clone();
    let platform_cell = platform_cell_clone.clone();
    create_game_api_function(
        &lua,
        "create_texture",
        move |_, (name, visual): (String, Table)| {
            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            // check if name already exists
            if board.contains_texture(&name) {
                return Err(mlua::Error::RuntimeError(format!(
                    "Attempted to create a texture with name {name}, which is already in use."
                )));
            }

            // try to parse the lua table
            let result = TextureIR::try_from_lua_table(&visual);
            if let Err(msg) = result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let visual = result.unwrap();

            // compile into a texture
            let mut platform = platform_cell
                .try_borrow_mut()
                .expect("platform is already borrowed");
            let mut texture_compiler = platform.texture_compiler();
            let texture_result =
                board.create_texture(&project_path, Some(name), &visual, &mut texture_compiler);

            match texture_result {
                Ok(_) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "create_piece", move |_, table: Table| {
        // try to parse the lua table
        let result = PieceIR::try_from_lua_table(&table);
        if let Err(msg) = result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let ir = result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        let piece_result = board.create_piece(ir);

        match piece_result {
            Ok(id) => Ok(id.id()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "create_pile", move |_, table: Table| {
        // try to parse the lua table
        let result = PileIR::try_from_lua_table(&table);
        if let Err(msg) = result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let ir = result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        let pile_result = board.create_pile(ir);

        match pile_result {
            Ok(id) => Ok(id.id()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "get_player_position",
        move |lua, player_val: Value| {
            // try to parse the entity
            let player_result = PlayerColor::try_from_lua_value(&player_val);
            if let Err(msg) = player_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let player = player_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.get_player_position(player) {
                Ok(p) => Ok(p.into_lua_table(lua)),
                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "move_player",
        move |_, (player_val, delta_val): (Value, Table)| {
            // try to parse the entity
            let player_result = PlayerColor::try_from_lua_value(&player_val);
            if let Err(msg) = player_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let player = player_result.unwrap();

            // try to parse the lua table
            let delta_result = finite_vec2_from_lua_table(&delta_val);
            if let Err(msg) = delta_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let delta = delta_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.move_player(player, delta) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_player_position",
        move |_, (player_val, position_val): (Value, Table)| {
            // try to parse the entity
            let player_result = PlayerColor::try_from_lua_value(&player_val);
            if let Err(msg) = player_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let player = player_result.unwrap();

            // try to parse the lua table
            let position_result = finite_vec2_from_lua_table(&position_val);
            if let Err(msg) = position_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let position = position_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_player_position(player, position) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_transform",
        move |_, (value, table): (Value, Table)| {
            // try to parse the entity
            let entity_result = EntityIdentifier::try_from_lua_value(&value);
            if let Err(msg) = entity_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let entity = entity_result.unwrap();

            // try to parse the lua table
            let object_result = RigidTransform::try_from_lua_table(&table);
            if let Err(msg) = object_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let mut object = object_result.unwrap();
            object.angle = object.angle.to_radians();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_transform(entity, object) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_position",
        move |_, (value, table): (Value, Table)| {
            // try to parse the entity
            let entity_result = EntityIdentifier::try_from_lua_value(&value);
            if let Err(msg) = entity_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let entity = entity_result.unwrap();

            // try to parse the lua table
            let object_result = finite_vec2_from_lua_table::<V>(&table);
            if let Err(msg) = object_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let object = object_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_position(entity, object) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "set_angle", move |_, (value, angle): (Value, f32)| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.set_angle(entity, angle.to_radians()) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_snap_offset",
        move |_, (value, table): (Value, Table)| {
            // try to parse the entity
            let entity_result = EntityIdentifier::try_from_lua_value(&value);
            if let Err(msg) = entity_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let entity = entity_result.unwrap();

            // try to parse the lua table
            let object_result = finite_vec2_from_lua_table::<V>(&table);
            if let Err(msg) = object_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let object = object_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_snap_offset(entity, object) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_snap_offset", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.get_snap_offset(entity) {
            Ok(offset) => Ok(offset.into_lua_table(lua)?),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_snap_radius",
        move |_, (value, radius): (Value, f32)| {
            // try to parse the entity
            let entity_result = EntityIdentifier::try_from_lua_value(&value);
            if let Err(msg) = entity_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let entity = entity_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_snap_radius(entity, radius) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_snap_radius", move |_, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.get_snap_radius(entity) {
            Ok(r) => Ok(r),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_transform", move |lua, item_val: Value| {
        // try to parse the entity
        let item_result = EntityIdentifier::try_from_lua_value(&item_val);
        if let Err(msg) = item_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let item = item_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.get_transform(item) {
            Ok(transform) => Ok(transform.into_lua_table(lua)?),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "center_item", move |_, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.center_item(entity) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "show_debug", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.show_debug(entity) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "hide_debug", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.hide_debug(entity) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "is_player_visible", move |lua, value: Value| {
        // try to parse the entity
        let player_result = PlayerColor::try_from_lua_value(&value);
        if let Err(msg) = player_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let player = player_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.is_player_visible(player) {
            Ok(b) => Ok(b),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "show_player", move |lua, value: Value| {
        // try to parse the entity
        let player_result = PlayerColor::try_from_lua_value(&value);
        if let Err(msg) = player_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let player = player_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.show_player(player) {
            Ok(b) => Ok(b),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "hide_player", move |lua, value: Value| {
        // try to parse the entity
        let player_result = PlayerColor::try_from_lua_value(&value);
        if let Err(msg) = player_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let player = player_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.hide_player(player) {
            Ok(b) => Ok(b),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "is_visible", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.is_visible(entity) {
            Ok(b) => Ok(b),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "show", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.show(entity) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "hide", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.hide(entity) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "disable_collisions", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.disable_collisions(entity) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "enable_collisions", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.enable_collisions(entity) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "is_piece", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.is_piece(entity))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "is_pile", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.is_piece(entity))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "is_held", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.is_held(entity))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "is_holding", move |lua, value: Value| {
        // try to parse the entity
        let player_result = PlayerColor::try_from_lua_value(&value);
        if let Err(msg) = player_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let player = player_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.is_holding(player))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_held_item",
        move |lua, (player_val, item_val): (Value, Value)| {
            // try to parse the player
            let player_result = PlayerColor::try_from_lua_value(&player_val);
            if let Err(msg) = player_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let player = player_result.unwrap();

            // try to parse the held item
            let item = if item_val.is_nil() {
                None
            } else {
                Some(match EntityIdentifier::try_from_lua_value(&item_val) {
                    Ok(e) => e,
                    Err(msg) => return Err(mlua::Error::RuntimeError(msg)),
                })
            };

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_held_item(player, item) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_held_item", move |lua, value: Value| {
        // try to parse the entity
        let player_result = PlayerColor::try_from_lua_value(&value);
        if let Err(msg) = player_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let player = player_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.get_held_item(player).map(|e| e.id()))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_holder", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.get_holder(entity).map(|c| c.name()))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "pile_top",
        move |lua, (pile_val, piece_val): (Value, Value)| {
            // try to parse the pile
            let pile_result = EntityIdentifier::try_from_lua_value(&pile_val);
            if let Err(msg) = pile_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let pile = pile_result.unwrap();

            // try to parse the piece
            let piece_result = EntityIdentifier::try_from_lua_value(&piece_val);
            if let Err(msg) = piece_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let piece = piece_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.pile_top(pile, piece) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "pile_bottom",
        move |lua, (pile_val, piece_val): (Value, Value)| {
            // try to parse the pile
            let pile_result = EntityIdentifier::try_from_lua_value(&pile_val);
            if let Err(msg) = pile_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let pile = pile_result.unwrap();

            // try to parse the piece
            let piece_result = EntityIdentifier::try_from_lua_value(&piece_val);
            if let Err(msg) = piece_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let piece = piece_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.pile_bottom(pile, piece) {
                Ok(()) => Ok(()),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "unpile", move |lua, piece_val: Value| {
        // try to parse the piece
        let piece_result = EntityIdentifier::try_from_lua_value(&piece_val);
        if let Err(msg) = piece_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let piece = piece_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.unpile(piece) {
            Ok(()) => Ok(()),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_item_overlaps", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.lua_query_overlaps(&lua, entity) {
            Ok(t) => Ok(t),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "get_top_piece_on_point",
        move |_, point_val: Value| {
            // try to parse the entity
            let point_result = finite_vec2_from_lua_value(&point_val);
            if let Err(msg) = point_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let point = point_result.unwrap();

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            Ok(board.lua_query_top_piece(point))
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_pieces_on_point", move |lua, point_val: Value| {
        // try to parse the entity
        let point_result = finite_vec2_from_lua_value(&point_val);
        if let Err(msg) = point_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let point = point_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.lua_query_point_overlaps(&lua, point))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_snap_piles", move |lua, player_val: Value| {
        // try to parse the entity
        let player_result = PlayerColor::try_from_lua_value(&player_val);
        if let Err(msg) = player_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let player = player_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.lua_query_snaps(&lua, player) {
            Ok(t) => Ok(t),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_snap_target",
        move |lua, (player_val, pile_val): (Value, Value)| {
            // try to parse the entity
            let player_result = PlayerColor::try_from_lua_value(&player_val);
            if let Err(msg) = player_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let player = player_result.unwrap();

            // try to parse the pile
            let pile = if pile_val.is_nil() {
                None
            } else {
                Some(match EntityIdentifier::try_from_lua_value(&pile_val) {
                    Ok(e) => e,
                    Err(msg) => return Err(mlua::Error::RuntimeError(msg)),
                })
            };

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_snap_target(player, pile) {
                Ok(t) => Ok(t),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "add_tags",
        move |lua, (item_val, tag_table): (Value, Table)| {
            // try to parse the piece
            let item_result = EntityIdentifier::try_from_lua_value(&item_val);
            if let Err(msg) = item_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let item = item_result.unwrap();

            let mut tags = Vec::new();
            for tag_result in tag_table.sequence_values::<Value>() {
                match String::try_from_lua_value(&tag_result?) {
                    Ok(tag) => tags.push(tag),
                    Err(msg) => return Err(mlua::Error::RuntimeError(msg)),
                }
            }

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.add_tags(item, tags.iter()) {
                Ok(t) => Ok(t),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "remove_tags",
        move |lua, (item_val, tag_table): (Value, Table)| {
            // try to parse the piece
            let item_result = EntityIdentifier::try_from_lua_value(&item_val);
            if let Err(msg) = item_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let item = item_result.unwrap();

            let mut tags = Vec::new();
            for tag_result in tag_table.sequence_values::<Value>() {
                match String::try_from_lua_value(&tag_result?) {
                    Ok(tag) => tags.push(tag),
                    Err(msg) => return Err(mlua::Error::RuntimeError(msg)),
                }
            }

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.remove_tags(item, tags.iter()) {
                Ok(t) => Ok(t),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "set_tags",
        move |lua, (item_val, tag_table): (Value, Table)| {
            // try to parse the piece
            let item_result = EntityIdentifier::try_from_lua_value(&item_val);
            if let Err(msg) = item_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let item = item_result.unwrap();

            let mut tags = Vec::new();
            for tag_result in tag_table.sequence_values::<Value>() {
                match String::try_from_lua_value(&tag_result?) {
                    Ok(tag) => tags.push(tag),
                    Err(msg) => return Err(mlua::Error::RuntimeError(msg)),
                }
            }

            let mut board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.set_tags(item, tags.iter()) {
                Ok(t) => Ok(t),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "clear_tags", move |lua, item_val: Value| {
        // try to parse the piece
        let item_result = EntityIdentifier::try_from_lua_value(&item_val);
        if let Err(msg) = item_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let item = item_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.clear_tags(item) {
            Ok(t) => Ok(t),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(
        &lua,
        "has_tag",
        move |lua, (item_val, tag_val): (Value, Value)| {
            // try to parse the piece
            let item_result = EntityIdentifier::try_from_lua_value(&item_val);
            if let Err(msg) = item_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let item = item_result.unwrap();

            // try to parse the tag
            let tag_result = String::try_from_lua_value(&tag_val);
            if let Err(msg) = tag_result {
                return Err(mlua::Error::RuntimeError(msg));
            }
            let tag = tag_result.unwrap();

            let board = board_cell
                .try_borrow_mut()
                .expect("board is already borrowed");

            match board.has_tag(item, &tag) {
                Ok(b) => Ok(b),

                Err(msg) => Err(mlua::Error::RuntimeError(msg)),
            }
        },
    );

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_items_with_tag", move |lua, tag_val: Value| {
        // try to parse the tag
        let tag_result = String::try_from_lua_value(&tag_val);
        if let Err(msg) = tag_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let query_tag = tag_result.unwrap();

        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        let iter = board.get_items_with_tag(&query_tag);
        let output_table = lua.create_table()?;
        for (idx, entity) in iter.enumerate() {
            output_table.raw_set(idx + 1, entity.id())?;
        }

        Ok(output_table)
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "delete", move |lua, value: Value| {
        // try to parse the entity
        let entity_result = EntityIdentifier::try_from_lua_value(&value);
        if let Err(msg) = entity_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let entity = entity_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        match board.delete_item(entity) {
            Ok(t) => Ok(t),

            Err(msg) => Err(mlua::Error::RuntimeError(msg)),
        }
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_piles", move |lua, _: ()| {
        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.lua_get_piles(lua))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_pieces", move |lua, _: ()| {
        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.lua_get_pieces(lua))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_items", move |lua, _: ()| {
        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        Ok(board.lua_get_items(lua))
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "set_background", move |lua, color_val: Value| {
        let color_result = color_from_lua_value::<C>(&color_val);
        if let Err(msg) = color_result {
            return Err(mlua::Error::RuntimeError(msg));
        }
        let color = color_result.unwrap();

        let mut board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");
        board.color = color;

        Ok(())
    });

    let board_cell = board_cell_clone.clone();
    create_game_api_function(&lua, "get_board_dimensions", move |lua, _: ()| {
        let board = board_cell
            .try_borrow_mut()
            .expect("board is already borrowed");

        board.dimensions().into_lua_table(&lua)
    });

    // load default update function to prevent immediate crash
    match lua.load("function update(_) end").exec() {
        Ok(()) => (),
        Err(msg) => panic!("Unable to load default functions: {msg}"),
    }

    lua
}
