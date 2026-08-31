use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use mlua::{
    Error, Lua, LuaOptions, MultiValue, StdLib, Table,
    Value::{self, Nil},
};
use notify::Watcher;
use raylib::ffi::{Color, Rectangle, Texture2D, Vector2};

use crate::{geometry::ConvexHull, piece_components::VisualInfo, resources::GameResources};

/// creates a lua instance with the libraries that gatheround allows
fn get_lua_instance() -> Lua {
    let exclude_libs = StdLib::IO | StdLib::OS | StdLib::COROUTINE;
    let libs = StdLib::ALL_SAFE & !exclude_libs;

    Lua::new_with(libs, LuaOptions::default()).expect("Unable to create lua instance.")
}

/// runs a game editor from the given project path
pub fn run_game_editor(project_path: &Path) {
    // initialize game resources
    let mut resource_cell = RefCell::new(GameResources::<
        Color,
        Rectangle,
        Vector2,
        ConvexHull<Vector2>,
        Texture2D,
        PathBuf,
        Texture2D,
    >::default());

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

    // setup the lua instance
    let lua = get_lua_instance();
    {
        let api = [
            // creates a new visual which can be retreived directly based on the given table
            // returns the id of the visual
            // may fail if the passed visual info is invalid or a visual with the given name already
            // exists
            (
                "new_visual",
                lua.create_function(|lua, (name, visual): (String, Table)| {
                    let mut output = MultiValue::new();

                    let result = VisualInfo::try_from_lua_table(visual);
                    if let Err(msg) = result {
                        output.push_back(Nil);
                        output.push_back(Value::String(lua.create_string(msg)?));
                        return Ok(output);
                    }
                    let visual = result.unwrap();

                    let mut resources = resource_cell
                        .try_borrow_mut()
                        .expect("resources are already borrowed");
                    if let Some(id) = resources.visuals.add_visual(name) {
                        output.push_back(Value::Integer(id.id() as i64));
                        Ok(output)
                    } else {
                        output.push_back(Nil);
                        output.push_back(Value::String(
                            lua.create_string(format!("Visual with name '{name}' already exists."))?
                        ));
                        Ok(output)
                    }
                })
                .unwrap(),
            ),
            // gets a copy of a visual as a table
            ("get_visual", lua.create_function(|_, ()| todo!()).unwrap()),
            // converts a visual into a texture, returning its id
            (
                "compile_visual",
                lua.create_function(|_, ()| todo!()).unwrap(),
            ),
        ];
    }

    // setup raylib instance
    let (mut rl, thread) = raylib::init()
        .resizable()
        .title("Gatheround Game Editor")
        .build();
    rl.set_target_fps(24);

    // run editor
    while !rl.window_should_close() {}
}
