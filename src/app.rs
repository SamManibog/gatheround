use std::{cell::RefCell, fmt::Debug, path::Path, rc::Rc};

use mlua::{Lua, MultiValue};

const MAIN_LUA_FILE: &str = "main.lua";

use crate::{
    board::BoardManager,
    cheat_buffer::CheatBuffer,
    color::IColor,
    geometry::{IConvexShape, IRect, IVec2},
    input::InputState,
    lua::{FromLuaTable, get_lua_instance},
    platform::{GlobalAssets, IPlatform, IRenderer},
    server::GameServer,
};

/// runs an app with the given settings
pub async fn run_app_with_settings<
    C: IColor + 'static,
    R: IRect + 'static,
    V: IVec2 + 'static,
    S: IConvexShape + FromLuaTable + Debug + From<R> + 'static,
    T: 'static,
    F: 'static,
    P: IPlatform<Texture = T, Font = F, Color = C, Rectangle = R, Vector2 = V, ConvexShape = S>,
>(
    project_path: &Path,
    host_address: &str,
    dimensions: V,
    mut platform: P,
) {
    // initialize input state
    let mut input_state = InputState::default();

    // load global assets
    let global_assets = GlobalAssets::new(&mut platform.texture_compiler());

    // initialize board
    let mut board = BoardManager::new(dimensions);
    board.lobby_mode(
        &mut platform.texture_compiler(),
        &format!("{host_address}{}", GameServer::MAIN_PATH),
    );
    let board_cell = Rc::new(RefCell::new(board));

    // initialize cheat buffer
    let mut cheat_buffer = CheatBuffer::new();

    let platform_cell = Rc::new(RefCell::new(platform));

    // get and run init function
    let lua: Lua = get_lua_instance(&project_path, &board_cell, &platform_cell);

    // load user script
    let main_path = project_path.join(MAIN_LUA_FILE);
    let script_content = std::fs::read_to_string(&main_path)
        .expect(&format!("Unable to read lua file '{:#?}'.", &main_path));
    let main_chunk = lua.load(script_content);

    let lua_update_callback = match main_chunk.eval::<mlua::Function>() {
        Ok(f) => f,
        Err(msg) => panic!(
            "Failed to run lua file '{:#?}':\n{}",
            &main_path,
            msg.to_string()
        ),
    };

    // initialize server
    let server = GameServer::run_new(host_address.to_string()).await;

    // run main loop
    loop {
        // detect window close
        if platform_cell.borrow_mut().should_close() {
            return;
        }

        // poll for new players
        {
            for player in server.poll_new_players() {
                if let Err(err) = board_cell.borrow_mut().add_player(player) {
                    eprintln!("{err}");
                }
                input_state.add_player(player);
            }
        }

        // run update cycle
        {
            let update_table = input_state.update(&lua, server.poll_inputs());
            if let Err(err) = lua_update_callback.call::<MultiValue>(update_table) {
                eprintln!("Got runtime error: {err}");
            }
        }

        // update commands
        {
            let delta_time = platform_cell.borrow_mut().delta_time();
            let cmd_opt = cheat_buffer.update(
                &mut platform_cell.borrow_mut().keyboard_reader(),
                delta_time,
            );
            if let Some(cmd) = cmd_opt {
                match lua.load(cmd).eval::<mlua::MultiValue>() {
                    Ok(v) => println!("GOTVALUE FROM COMMAND!: {:?}", v),
                    Err(e) => {
                        eprintln!("GOT ERROR FROM COMMAND! NOT YET SURE WHAT TO DO WITH IT:\n{e}")
                    }
                }
            }
        }

        // run draw cycle
        {
            let board = board_cell.borrow();
            platform_cell
                .borrow_mut()
                .renderer()
                .draw_app(&board, &cheat_buffer, &global_assets);
        }
    }
}
