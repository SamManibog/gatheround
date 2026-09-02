use std::{cell::RefCell, path::Path, rc::Rc, sync::mpsc};

use mlua::{
    Lua, LuaOptions, MultiValue, StdLib, Table,
    Value::{self, Nil},
};
use notify::Watcher;
use raylib::{
    ffi::{Color, Rectangle, Vector2},
    text::Font,
    texture::Texture2D,
};

use crate::{
    geometry::{ConvexHull, IVec2},
    piece_components::VisualInfo,
    raylib_integrations::RaylibRenderer,
};

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
                Err(msg) => return Err(format!("Invalid {value_name} at index {}: {msg}", index + 1)),
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

/// runs a game editor from the given project path
pub fn run_game(project_path: &Path) {
    // initialize game resources
    let resource_cell = Rc::new(RefCell::new(GameResources::<
        Vector2,
        ConvexHull<Vector2>,
        Texture2D,
        Font,
    >::default()));

    // setup raylib instance
    let (mut rl, thread) = raylib::init()
        .resizable()
        .title("Gatheround Game Editor")
        .build();
    rl.set_target_fps(24);
    let rl_cell = Rc::new(RefCell::new(rl));
    let thread_cell = Rc::new(RefCell::new(thread));

    // setup the lua instance
    let lua = get_lua_instance();
    {
        let resources1 = resource_cell.clone();
        let rl1 = rl_cell.clone();
        let thread1 = thread_cell.clone();

        let api = [
            // creates a new texture resource based on the given list
            // on success returns the id of the texture or nil, error in case of failure
            (
                "create_texture",
                lua.create_function(move |lua, (visual, name_opt): (Table, Option<String>)| {
                    let mut output = MultiValue::new();

                    let mut resources = resources1.try_borrow_mut().expect("resources are already borrowed");

                    // check if name already exists
                    if let Some(name) = &name_opt {
                        if resources.textures.contains_texture_name(name) {
                            output.push_back(Nil);
                            output.push_back(Value::String(lua.create_string(&format!("Attempted to create a texture with name {name}, which is already in use."))?));
                            return Ok(output);
                        }
                    }

                    // try to parse the lua table
                    let result = VisualInfo::<Color, Rectangle, Vector2>::try_from_lua_table(&visual);
                    if let Err(msg) = result {
                        output.push_back(Nil);
                        output.push_back(Value::String(lua.create_string(msg)?));
                        return Ok(output);
                    }
                    let visual = result.unwrap();


                    // compile into a texture
                    let mut rl = rl1.try_borrow_mut().expect("rl_cell is already borrowed");
                    let thread = thread1.try_borrow_mut().expect("thread_cell is already borrowed");
                    let mut renderer = RaylibRenderer {rl: &mut rl, thread: &thread};

                    let texture_result = resources.textures.load_texture(name_opt, &visual, Vector2::from_xy(800.0, 800.0), &mut renderer);

                    match texture_result {
                        Ok(id) => {
                        output.push_back(Value::Integer(id.id() as i64));
                        Ok(output)
                        }

                        Err(msg) => {
                            output.push_back(Nil);
                            output.push_back(Value::String(
                                lua.create_string(msg)?
                            ));
                            Ok(output)
                        }
                    }
                })
                .unwrap(),
            ),

        ];

        for (name, function) in api {
            lua.globals()
                .raw_set(name, function)
                .expect(&format!("Unable to create global function '{name}'"));
        }
    }

    // set up a file watch on project directory to determine when reloads must occur
    let (notify_sender, notify_receiver) = mpsc::channel();
    let mut file_watcher = notify::recommended_watcher(notify_sender).expect(&format!(
        "Could not obtain a file watcher for '{}'.",
        project_path.to_string_lossy()
    ));
    file_watcher
        .watch(project_path, notify::RecursiveMode::Recursive)
        .expect(&format!(
            "Could not watch '{}'.",
            project_path.to_string_lossy()
        ));

    // run editor
    loop {
        let rl = rl_cell
            .try_borrow_mut()
            .expect("raylib_cell is already borrowed mutably.");
        if rl.window_should_close() {
            return;
        }
        drop(rl);

        todo!()
    }
}
