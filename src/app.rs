use std::{
    cell::RefCell,
    fmt::Debug,
    path::PathBuf,
    rc::Rc,
};

use mlua::{Lua, MultiValue};

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

#[derive(Debug)]
pub struct AppState<
    C: IColor + 'static,
    R: IRect + 'static,
    V: IVec2 + 'static,
    S: IConvexShape + FromLuaTable + Debug + From<R> + 'static,
    T: 'static,
    F: 'static,
    P: IPlatform<Texture = T, Font = F, Color = C, Rectangle = R, Vector2 = V, ConvexShape = S>,
> {
    lua_state: Option<(Lua, mlua::Function)>,
    global_assets: GlobalAssets<T, F>,
    board_cell: Rc<RefCell<BoardManager<C, R, V, S, T, F>>>,
    cheat_buffer: CheatBuffer,
    platform_cell: Rc<RefCell<P>>,
    server: GameServer,
    input_state: InputState,
    project_path: PathBuf,
    host_address: String,
}

impl<
    C: IColor + 'static,
    R: IRect + 'static,
    V: IVec2 + 'static,
    S: IConvexShape + FromLuaTable + Debug + From<R> + 'static,
    T: 'static,
    F: 'static,
    P: IPlatform<Texture = T, Font = F, Color = C, Rectangle = R, Vector2 = V, ConvexShape = S>,
> AppState<C, R, V, S, T, F, P>
{
    // the name of the lua file to load when initializing lua state
    const MAIN_LUA_FILE: &str = "main.lua";

    pub async fn new_with_settings(
        project_path: PathBuf,
        host_address: String,
        dimensions: V,
        mut platform: P,
    ) -> Self {
        // initialize input state
        let input_state = InputState::default();

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
        let cheat_buffer = CheatBuffer::new();

        let platform_cell = Rc::new(RefCell::new(platform));

        // initialize server
        let server = GameServer::run_new(host_address.to_string()).await;

        Self {
            lua_state: None,
            cheat_buffer,
            global_assets,
            board_cell,
            platform_cell,
            server,
            input_state,
            host_address,
            project_path,
        }
    }

    // ends lobby state and starts a new game
    pub fn start_game(&mut self, project_path: PathBuf) {
        // reset board
        self.board_cell.borrow_mut().reset();

        // load lua state for the game
        let lua: Lua = get_lua_instance(&project_path, &self.board_cell, &self.platform_cell);
        let main_path = project_path.join(Self::MAIN_LUA_FILE);
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

        self.lua_state = Some((lua, lua_update_callback));

        // prevent new players from joining
        self.server.block_new_players();

        // discard old input events
        for _ in self.server.poll_inputs() {}
    }

    // ends game state and goes into lobby mode
    // note that AppState is constructed in lobby mode
    pub fn start_lobby(&mut self) {
        // reset board
        let mut board = self.board_cell.borrow_mut();
        board.reset();
        board.lobby_mode(
            &mut self.platform_cell.borrow_mut().texture_compiler(),
            &format!("{}{}", self.host_address, GameServer::MAIN_PATH),
        );

        // clear lua state
        self.lua_state = None;

        // clear cheat buffer
        self.cheat_buffer.clear();

        // allow new players to join
        self.server.allow_new_players();

        // discard old input events
        for _ in self.server.poll_inputs() {}
    }

    // handle lobby-specific update functionality
    fn update_lobby_mode(&mut self) {
        for player in self.server.poll_new_players() {
            self.board_cell.borrow_mut().lobby_add_player(player);
            self.input_state.add_player(player);
        }

        self.board_cell
            .borrow_mut()
            .lobby_handle_inputs(self.server.poll_inputs());
    }

    // handle game-specific update functionality
    fn update_game_mode(&mut self) {
        let (lua, lua_update_callback) = self.lua_state.as_mut().unwrap();

        // run lua-defined update cycle
        if let Err(err) = lua_update_callback
            .call::<MultiValue>(self.input_state.update(&lua, self.server.poll_inputs()))
        {
            eprintln!("Got runtime error: {err}");
        }

        // run cheats
        {
            let delta_time = self.platform_cell.borrow_mut().delta_time();
            let cmd_opt = self.cheat_buffer.update(
                delta_time,
                &mut self.platform_cell.borrow_mut().keyboard_reader(),
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
    }

    // run the entire update cycle once
    // returns false when the app should close
    fn update(&mut self) -> bool {
        if self.platform_cell.borrow_mut().should_close() {
            return false;
        }

        if self.lua_state.is_some() {
            self.update_game_mode();
        } else {
            self.update_lobby_mode();
        }

        // run draw cycle
        self.platform_cell.borrow_mut().renderer().draw_app(
            &self.board_cell.borrow(),
            &self.cheat_buffer,
            &self.global_assets,
        );

        // signal to continue running
        true
    }

    /// runs the app
    pub fn run(&mut self) {
        while self.update() {}
    }
}
