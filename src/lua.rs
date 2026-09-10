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
    board::BoardManager,
    ecs::EntityTrait,
    geometry::{ConvexHull, IConvexShape, IRect, IVec2, RigidTransform, vec2_from_lua_table, vec2_from_lua_value},
    intermediate_representations::{EntityIdentifier, PieceIR, PileIR, VisualInfo},
    raylib_integrations::RaylibTextureCompiler, resources::PlayerColor,
};

const MAIN_LUA_FILE: &str = "main.lua";

const DEFAULT_LUA_SCRIPT: &str = include_str!("defaults.lua");

/// for structs that can be created based off of a lua table
pub trait FromLuaTable: Sized {
    fn try_from_lua_table(table: &Table) -> Result<Self, String>;
}

/// for structs that can be created based off of a lua value
pub trait FromLuaValue: Sized {
    fn try_from_lua_value(value: &Value) -> Result<Self, String>;
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
fn create_game_api_function<F1, F2, A, B, R, V, S, T, F>(
    lua: &Lua,
    project_path: &Path,
    board: &Rc<RefCell<BoardManager<R, V, S, T, F>>>,
    rl: &Rc<RefCell<RaylibHandle>>,
    thread: &Rc<RefCell<RaylibThread>>,
    name: &str,
    func_creator: F1,
) where
    F1: Fn(
        PathBuf,
        Rc<RefCell<BoardManager<R, V, S, T, F>>>,
        Rc<RefCell<RaylibHandle>>,
        Rc<RefCell<RaylibThread>>,
    ) -> F2,
    F2: Fn(&Lua, A) -> mlua::Result<B> + mlua::MaybeSend + 'static,
    A: mlua::FromLuaMulti,
    B: mlua::IntoLuaMulti,
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
        RefCell<BoardManager<Rectangle, Vector2, ConvexHull<Vector2>, RenderTexture2D, Font>>,
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
                let position_result = vec2_from_lua_table(&position_val);
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

                match board.lua_get_player_position(&lua, player) {
                    Ok(p) => Ok(p),
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
                let object_result = vec2_from_lua_table::<Vector2>(&table);
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
                let point_result = vec2_from_lua_value(&point_val);
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
                let point_result = vec2_from_lua_value(&point_val);
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

    // load default functions
    match lua.load(DEFAULT_LUA_SCRIPT).exec() {
        Ok(()) => (),
        Err(msg) => panic!("Unable to load default functions: {msg}"),
    }

    // load user script
    let main_path = project_path.join(MAIN_LUA_FILE);
    let script_content = std::fs::read_to_string(&main_path)
        .expect(&format!("Unable to read lua file '{:#?}'.", &main_path));
    let main_chunk = lua.load(script_content);
    if let Err(msg) = main_chunk.exec() {
        panic!("Failed to run lua file '{:#?}':\n{msg}", &main_path,);
    }

    lua
}
