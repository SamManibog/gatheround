use std::{
    cell::RefCell,
    fmt::Debug,
    path::{Path, PathBuf},
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
    platform::{GlobalAssets, IPlatform, IRenderer, ITextureCompiler},
    server::GameServer,
};

#[derive(Debug, Clone)]
pub struct GameEntry<T> {
    pub title: String,
    pub directory: PathBuf,
    pub thumbnail: Option<T>,
}

#[derive(Debug)]
enum BoardState {
    Game {
        lua: Lua,
        update_callback: mlua::Function,
    },
    Lobby,
    GameSelector,
}

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
    board_state: BoardState,
    global_assets: GlobalAssets<T, F>,
    board_cell: Rc<RefCell<BoardManager<C, R, V, S, T, F>>>,
    cheat_buffer: CheatBuffer,
    platform_cell: Rc<RefCell<P>>,
    server: GameServer,
    input_state: InputState,
    game_dir: PathBuf,
    host_address: String,
}

// the name of the lua file to load when initializing lua state
pub const MAIN_LUA_FILE: &str = "main.lua";

// the name of the thumbnail
pub const THUMBNAIL_FILE: &str = "thumbnail.png";

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
        board.add_player(crate::resources::PlayerColor::Red);
        let entries = 
            Self::get_game_entries(&PathBuf::from(""), platform.texture_compiler());
        board.game_selector_mode(
            &mut platform.texture_compiler(),
            crate::resources::PlayerColor::Red,
            entries.into_iter(),
        );
        println!("board initialized in selector mode, remove this behavior");
        /*
                board.lobby_mode(
                    &mut platform.texture_compiler(),
                    &format!("{host_address}{}", GameServer::MAIN_PATH),
                );
        */
        let board_cell = Rc::new(RefCell::new(board));

        // initialize cheat buffer
        let cheat_buffer = CheatBuffer::new();

        let platform_cell = Rc::new(RefCell::new(platform));

        // initialize server
        let server = GameServer::run_new(host_address.to_string()).await;

        println!("app made in selector mode");
        Self {
            board_state: BoardState::GameSelector,
            cheat_buffer,
            global_assets,
            board_cell,
            platform_cell,
            server,
            input_state,
            host_address,
            game_dir: project_path,
        }
    }

    // ends lobby state and starts a new game
    pub fn start_game(&mut self, project_path: PathBuf) {
        // reset board
        self.board_cell.borrow_mut().reset();

        // load lua state for the game
        let lua: Lua = get_lua_instance(&project_path, &self.board_cell, &self.platform_cell);
        let main_path = project_path.join(MAIN_LUA_FILE);
        let script_content = std::fs::read_to_string(&main_path)
            .expect(&format!("Unable to read lua file '{:#?}'.", &main_path));
        let main_chunk = lua.load(script_content);

        let update_callback = match main_chunk.eval::<mlua::Function>() {
            Ok(f) => f,
            Err(msg) => panic!(
                "Failed to run lua file '{:#?}':\n{}",
                &main_path,
                msg.to_string()
            ),
        };

        self.board_state = BoardState::Game {
            lua,
            update_callback,
        };

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
        self.board_state = BoardState::Lobby;

        // clear cheat buffer
        self.cheat_buffer.clear();

        // allow new players to join
        self.server.allow_new_players();

        // discard old input events
        for _ in self.server.poll_inputs() {}
    }

    // gets a list of game entries
    fn get_game_entries(
        game_dir: &Path,
        mut texture_compiler: P::TextureCompiler<'_>,
    ) -> Vec<GameEntry<P::Texture>> {
        vec![
            GameEntry {
                title: String::from("1"),
                directory: PathBuf::from("1"),
                thumbnail: Some(
                    texture_compiler
                        .load_texture_from_memory(".png", crate::platform::OPEN_HAND_BYTES)
                        .unwrap(),
                ),
            },
            GameEntry {
                title: String::from("2"),
                directory: PathBuf::from("2"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("3"),
                directory: PathBuf::from("3"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("4"),
                directory: PathBuf::from("4"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("5"),
                directory: PathBuf::from("5"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("6"),
                directory: PathBuf::from("6"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("7"),
                directory: PathBuf::from("7"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("8"),
                directory: PathBuf::from("8"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("9"),
                directory: PathBuf::from("9"),
                thumbnail: None,
            },
            GameEntry {
                title: String::from("10"),
                directory: PathBuf::from("10"),
                thumbnail: None,
            },
        ]
    }

    // enters game selector mode
    pub fn start_game_selector(&mut self) {
        // reset board
        let mut board = self.board_cell.borrow_mut();
        board.reset();
        board.game_selector_mode(
            &mut self.platform_cell.borrow_mut().texture_compiler(),
            crate::resources::PlayerColor::Red,
            Self::get_game_entries(
                &self.game_dir,
                self.platform_cell.borrow_mut().texture_compiler(),
            )
            .into_iter(),
        );

        // clear lua state
        self.board_state = BoardState::GameSelector;

        // clear cheat buffer
        self.cheat_buffer.clear();

        // block new players from joining
        //self.server.block_new_players();
        println!("new players not blocked, change this behavior");

        // discard old input events
        for _ in self.server.poll_inputs() {}
    }

    // handle lobby-specific update functionality
    fn update_selector_mode(&mut self) {
        if let Some(path) = self
            .board_cell
            .borrow_mut()
            .selector_handle_inputs(self.server.poll_inputs())
        {
            println!("Game Path: {} selected!", path.to_string_lossy());
        }
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
        let (lua, lua_update_callback) = match &mut self.board_state {
            BoardState::Game {
                lua,
                update_callback,
            } => (lua, update_callback),
            _ => unreachable!(),
        };

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

        match self.board_state {
            BoardState::Game { .. } => self.update_game_mode(),
            BoardState::Lobby => self.update_lobby_mode(),
            BoardState::GameSelector => self.update_selector_mode(),
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
