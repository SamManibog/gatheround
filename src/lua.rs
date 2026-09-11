use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use mlua::{Lua, LuaOptions, StdLib, Table, Value};
use raylib::{
    RaylibHandle, RaylibThread,
    ffi::{Color, Rectangle, Vector2},
    text::Font,
    texture::RenderTexture2D,
};

use crate::{
    board::BoardManager, color::{IColor, color_from_lua_value}, ecs::EntityTrait, geometry::{
        ConvexHull, IConvexShape, IRect, IVec2, RigidTransform, finite_vec2_from_lua_table,
        finite_vec2_from_lua_value,
    }, intermediate_representations::{EntityIdentifier, PieceIR, PileIR, VisualInfo}, raylib_integrations::RaylibTextureCompiler, resources::PlayerColor
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
                return Err(format!("non-finite value."))
            } else {
                Ok(*number as f32)
            }
        },

        Value::Number(number) => {
            let min = f32::MIN as f64;
            let max = f32::MAX as f64;

            if !number.is_finite() || *number < min || *number > max {
                return Err(format!("non-finite value."))
            } else {
                Ok(*number as f32)
            }
        },

        _ => Err(format!("non-numeric type."))
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

/// creates a lua instance with the libraries that gatheround allows
fn get_lua_instance() -> Lua {
    let exclude_libs = StdLib::IO | StdLib::OS | StdLib::COROUTINE;
    let libs = StdLib::ALL_SAFE & !exclude_libs;

    Lua::new_with(libs, LuaOptions::default()).expect("Unable to create lua instance.")
}

#[inline]
fn create_game_api_function<F1, F2, A, B, C, R, V, S, T, F>(
    lua: &Lua,
    project_path: &Path,
    board: &Rc<RefCell<BoardManager<C, R, V, S, T, F>>>,
    rl: &Rc<RefCell<RaylibHandle>>,
    thread: &Rc<RefCell<RaylibThread>>,
    name: &str,
    func_creator: F1,
) where
    F1: Fn(
        PathBuf,
        Rc<RefCell<BoardManager<C, R, V, S, T, F>>>,
        Rc<RefCell<RaylibHandle>>,
        Rc<RefCell<RaylibThread>>,
    ) -> F2,
    F2: Fn(&Lua, A) -> mlua::Result<B> + mlua::MaybeSend + 'static,
    A: mlua::FromLuaMulti,
    B: mlua::IntoLuaMulti,
    C: IColor,
    R: IRect,
    V: IVec2,
    S: IConvexShape + FromLuaTable,
{
    let function = lua
        .create_function_mut(func_creator(
            project_path.to_path_buf(),
            board.clone(),
            rl.clone(),
            thread.clone(),
        ))
        .expect(&format!("Unable to create global function '{name}'"));
    lua.globals()
        .raw_set(name, function)
        .expect(&format!("Unable to create global function '{name}'"));
}

macro_rules! define_game_api {
    ($name:expr, $lua:ident, $project_path:ident, $board_cell:ident, $rl_cell:ident, $thread_cell:ident, $body:block) => {
        create_game_api_function(
            &$lua,
            &$project_path,
            &$board_cell,
            &$rl_cell,
            &$thread_cell,
            $name,
            |$project_path, $board_cell, $rl_cell, $thread_cell| $body,
        );
    };
}

/// gets a lua instance with the proper game api
#[allow(unused_variables)]
pub fn get_game_lua_instance(
    project_path: &Path,
    board_cell: &Rc<
        RefCell<
            BoardManager<
                raylib::ffi::Color,
                Rectangle,
                Vector2,
                ConvexHull<Vector2>,
                RenderTexture2D,
                Font,
            >,
        >,
    >,
    rl_cell: &Rc<RefCell<RaylibHandle>>,
    thread_cell: &Rc<RefCell<RaylibThread>>,
) -> Lua {
    // setup the lua instance
    //
    // use the following template to define new functions
    //  define_game_api!("...", lua, project_path, board_cell, rl_cell, thread_cell, {
    //      move |_, ...: ...| {
    //          // function body
    //      }
    //  })
    let lua = get_lua_instance();
    define_game_api!(
        "create_texture",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
                let result = VisualInfo::<Color, Rectangle, Vector2>::try_from_lua_table(&visual);
                if let Err(msg) = result {
                    return Err(mlua::Error::RuntimeError(msg));
                }
                let visual = result.unwrap();

                // compile into a texture
                let mut rl = rl_cell
                    .try_borrow_mut()
                    .expect("rl_cell is already borrowed");
                let thread = thread_cell
                    .try_borrow_mut()
                    .expect("thread_cell is already borrowed");
                let mut renderer = RaylibTextureCompiler {
                    rl: &mut rl,
                    thread: &thread,
                };

                let magnification_dimensions = board.dimensions().scaled(0.8);
                let texture_result = board.create_texture(
                    &project_path,
                    name,
                    &visual,
                    magnification_dimensions,
                    &mut renderer,
                );

                match texture_result {
                    Ok(_) => Ok(()),

                    Err(msg) => Err(mlua::Error::RuntimeError(msg)),
                }
            }
        }
    );

    define_game_api!(
        "create_piece",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |_, table: Table| {
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
            }
        }
    );

    define_game_api!(
        "create_pile",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |_, table: Table| {
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
            }
        }
    );

    define_game_api!(
        "get_player_position",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "move_player",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "set_player_position",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "set_transform",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "set_position",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |_, (value, table): (Value, Table)| {
                // try to parse the entity
                let entity_result = EntityIdentifier::try_from_lua_value(&value);
                if let Err(msg) = entity_result {
                    return Err(mlua::Error::RuntimeError(msg));
                }
                let entity = entity_result.unwrap();

                // try to parse the lua table
                let object_result = finite_vec2_from_lua_table::<Vector2>(&table);
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
            }
        }
    );

    define_game_api!(
        "set_angle",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |_, (value, angle): (Value, f32)| {
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
            }
        }
    );

    define_game_api!(
        "set_snap_offset",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |_, (value, table): (Value, Table)| {
                // try to parse the entity
                let entity_result = EntityIdentifier::try_from_lua_value(&value);
                if let Err(msg) = entity_result {
                    return Err(mlua::Error::RuntimeError(msg));
                }
                let entity = entity_result.unwrap();

                // try to parse the lua table
                let object_result = finite_vec2_from_lua_table::<Vector2>(&table);
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
            }
        }
    );

    define_game_api!(
        "get_snap_offset",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "set_snap_radius",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "get_snap_radius",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |_, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "get_transform",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, item_val: Value| {
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
            }
        }
    );

    define_game_api!(
        "center_item",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |_, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "get_texture",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
                // try to parse the entity
                let entity_result = EntityIdentifier::try_from_lua_value(&value);
                if let Err(msg) = entity_result {
                    return Err(mlua::Error::RuntimeError(msg));
                }
                let entity = entity_result.unwrap();

                let board = board_cell
                    .try_borrow_mut()
                    .expect("board is already borrowed");

                match board.get_texture(entity) {
                    Ok(tex) => Ok(tex.map(|s| lua.create_string(s).unwrap())),

                    Err(msg) => Err(mlua::Error::RuntimeError(msg)),
                }
            }
        }
    );

    define_game_api!(
        "show_debug",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "hide_debug",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "show",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "hide",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "disable_collisions",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "enable_collisions",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "is_piece",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "is_pile",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "is_held",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "is_holding",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "set_held_item",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "get_held_item",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "get_holder",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "pile_top",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "pile_bottom",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "unpile",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, piece_val: Value| {
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
            }
        }
    );

    define_game_api!(
        "get_item_overlaps",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "get_top_piece_on_point",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "get_pieces_on_point",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, point_val: Value| {
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
            }
        }
    );

    define_game_api!(
        "get_snap_piles",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, player_val: Value| {
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
            }
        }
    );

    define_game_api!(
        "set_snap_target",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "add_tags",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "remove_tags",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "set_tags",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "clear_tags",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, item_val: Value| {
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
            }
        }
    );

    define_game_api!(
        "has_tag",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
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
            }
        }
    );

    define_game_api!(
        "get_items_with_tag",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, tag_val: Value| {
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
            }
        }
    );

    define_game_api!(
        "delete",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, value: Value| {
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
            }
        }
    );

    define_game_api!(
        "set_background",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, color_val: Value| {
                let color_result = color_from_lua_value::<raylib::ffi::Color>(&color_val);
                if let Err(msg) = color_result {
                    return Err(mlua::Error::RuntimeError(msg));
                }
                let color = color_result.unwrap();

                let mut board = board_cell
                    .try_borrow_mut()
                    .expect("board is already borrowed");
                board.color = color;

                Ok(())
            }
        }
    );

    define_game_api!(
        "toggle_fullscreen",
        lua,
        project_path,
        board_cell,
        rl_cell,
        thread_cell,
        {
            move |lua, _: ()| {
                let mut rl = rl_cell.borrow_mut();
                rl.toggle_fullscreen();
                Ok(())
            }
        }
    );

    // load default update function to prevent immediate crash
    match lua.load("function update(_) end").exec() {
        Ok(()) => (),
        Err(msg) => panic!("Unable to load default functions: {msg}"),
    }

    lua
}
