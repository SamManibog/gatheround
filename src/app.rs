use std::{cell::RefCell, fmt::Debug, path::PathBuf, rc::Rc};

use mlua::{Lua, MultiValue};

use crate::{
    assets,
    board::{BoardManager, players::PlayerColor},
    cheat_buffer::CheatBuffer,
    game::{self},
    input::InputState,
    lua::{FromLuaTable, get_lua_instance},
    platform::{IPlatform, IRenderer, ITextureCompiler},
    server::{GameServer, HostCommand, PlayerEvent},
    utils::{
        color::IColor,
        geometry::{IConvexShape, IRect, IVec2},
    },
};

#[derive(Debug)]
enum BoardMode {
    Game {
        lua: Lua,
        update_callback: mlua::Function,
    },
    Lobby,
    GameSelector,
}

impl BoardMode {
    pub fn is_lobby_mode(&self) -> bool {
        match self {
            Self::Lobby => true,
            _ => false,
        }
    }

    pub fn is_selector_mode(&self) -> bool {
        match self {
            Self::GameSelector => true,
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct GlobalAssets<T, F> {
    pub open_hand_texture: T,
    pub closed_hand_texture: T,
    pub cheat_buffer_font: F,
}

impl<T: 'static, F: 'static> GlobalAssets<T, F> {
    pub const CHEAT_BUFFER_FONT_SIZE: f32 = 28.0;

    pub fn new<R: ITextureCompiler<Texture = T, Font = F>>(compiler: &mut R) -> Self {
        let open_hand_texture = match compiler
            .load_texture_from_memory(assets::OPEN_HAND.filetype, assets::OPEN_HAND.data)
        {
            Ok(t) => t,
            Err(msg) => panic!("Unable to load open hand texture: {msg}"),
        };
        let closed_hand_texture = match compiler
            .load_texture_from_memory(assets::CLOSED_HAND.filetype, assets::CLOSED_HAND.data)
        {
            Ok(t) => t,
            Err(msg) => panic!("Unable to load open hand texture: {msg}"),
        };

        let font_data = assets::JETBRAINS_MONO;
        let cheat_buffer_font = match compiler.load_font_from_memory(
            font_data.filetype,
            font_data.data,
            Self::CHEAT_BUFFER_FONT_SIZE as u32,
            assets::ASCII_GRAPHIC_CHARSET,
        ) {
            Ok(t) => t,
            Err(msg) => panic!("Unable to load open hand texture: {msg}"),
        };

        Self {
            open_hand_texture,
            closed_hand_texture,
            cheat_buffer_font,
        }
    }
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
    host_player: Option<PlayerColor>,

    board_mode: BoardMode,
    global_assets: GlobalAssets<T, F>,
    board_cell: Rc<RefCell<BoardManager<C, R, V, S, T, F>>>,
    cheat_buffer: CheatBuffer,
    platform_cell: Rc<RefCell<P>>,
    server: GameServer,
    input_state: InputState,
    settings: GameSettings<V>,
}

#[derive(Debug, Clone)]
pub struct GameSettings<V: IVec2> {
    pub games_directory: PathBuf,
    pub ip_address: String,
    pub dimensions: V,
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
    pub async fn new_with_settings<G: FnOnce(&GameSettings<V>) -> P>(
        settings: GameSettings<V>,
        platform_generator: G,
    ) -> Self {
        // initialize server
        let server = GameServer::run_new(settings.ip_address.to_string()).await;

        // initialize platform
        let mut platform = platform_generator(&settings);

        // initialize input state
        let input_state = InputState::default();

        // load global assets
        let global_assets = GlobalAssets::new(&mut platform.texture_compiler());

        // initialize board
        let board_cell = Rc::new(RefCell::new(BoardManager::new(settings.dimensions)));

        // initialize cheat buffer
        let cheat_buffer = CheatBuffer::new();

        let platform_cell = Rc::new(RefCell::new(platform));

        Self {
            host_player: None,

            board_mode: BoardMode::Lobby,
            cheat_buffer,
            global_assets,
            board_cell,
            platform_cell,
            server,
            input_state,
            settings,
        }
    }

    // ends lobby state and starts a new game
    pub fn start_game(&mut self, project_path: PathBuf) {
        // reset board
        self.board_cell.borrow_mut().reset();

        // load lua state for the game
        let lua: Lua = get_lua_instance(&project_path, &self.board_cell, &self.platform_cell);
        let main_path = project_path.join(game::MAIN_LUA_FILE);
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

        self.board_mode = BoardMode::Game {
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
            &format!(
                "{}{}",
                self.settings.ip_address,
                GameServer::CONTROLLER_PATH
            ),
        );

        // clear lua state
        self.board_mode = BoardMode::Lobby;

        // clear cheat buffer
        self.cheat_buffer.clear();

        // allow new players to join
        self.server.allow_new_players();

        // discard old input events
        for _ in self.server.poll_inputs() {}
    }

    // enters game selector mode
    pub fn start_game_selector(&mut self) {
        // load entries
        let game_entries = game::load_game_entries(
            &self.settings.games_directory,
            &mut self.platform_cell.borrow_mut().texture_compiler(),
        );

        // reset board
        let mut board = self.board_cell.borrow_mut();
        board.reset();
        let picking_player = {
            if let Some(player) = self.host_player {
                player
            } else {
                board.player_iter().next().unwrap_or(PlayerColor::Red)
            }
        };

        board.game_selector_mode(
            &mut self.platform_cell.borrow_mut().texture_compiler(),
            picking_player,
            game_entries.into_iter(),
        );

        // clear lua state
        self.board_mode = BoardMode::GameSelector;

        // clear cheat buffer
        self.cheat_buffer.clear();

        // block new players from joining
        self.server.block_new_players();

        // discard old input events
        for _ in self.server.poll_inputs() {}
    }

    // handle lobby-specific update functionality
    fn update_selector_mode(&mut self) {
        for player_event in self.server.poll_player_events() {
            match player_event {
                PlayerEvent::Joined(_) => (),
                PlayerEvent::SetHost(player) => self.host_player = Some(player),
            }
        }

        let path_opt = self
            .board_cell
            .borrow_mut()
            .selector_handle_inputs(self.server.poll_inputs());
        if let Some(path) = path_opt {
            self.start_game(path);
        }
    }

    // handle lobby-specific update functionality
    fn update_lobby_mode(&mut self) {
        for player_event in self.server.poll_player_events() {
            match player_event {
                PlayerEvent::Joined(player) => {
                    self.board_cell.borrow_mut().lobby_add_player(player);
                    self.input_state.add_player(player);
                }

                PlayerEvent::SetHost(player) => self.host_player = Some(player),
            }
        }

        self.board_cell
            .borrow_mut()
            .lobby_handle_inputs(self.server.poll_inputs());
    }

    // handle game-specific update functionality
    fn update_game_mode(&mut self) {
        for player_event in self.server.poll_player_events() {
            match player_event {
                PlayerEvent::Joined(_) => (),
                PlayerEvent::SetHost(player) => self.host_player = Some(player),
            }
        }

        let (lua, lua_update_callback) = match &mut self.board_mode {
            BoardMode::Game {
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

        // poll for host commands
        let cmds: Vec<HostCommand> = self.server.poll_host_commands().collect();
        for cmd in cmds {
            match cmd {
                HostCommand::EnterGameSelectionMode => {
                    let has_sufficient_players = self.board_cell.borrow_mut().player_count() >= 1;
                    if !self.board_mode.is_selector_mode() && has_sufficient_players {
                        self.start_game_selector();
                    }
                }
                HostCommand::EnterLobbyMode => {
                    if !self.board_mode.is_lobby_mode() {
                        self.start_lobby();
                    }
                }
            }
        }

        match self.board_mode {
            BoardMode::Game { .. } => self.update_game_mode(),
            BoardMode::Lobby => self.update_lobby_mode(),
            BoardMode::GameSelector => self.update_selector_mode(),
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
        self.start_lobby();

        while self.update() {}
    }
}
