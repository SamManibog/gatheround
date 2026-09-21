use std::{
    cell::Cell,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    rc::Rc,
};

use serde::Deserialize;

use crate::platform::ITextureCompiler;

// the name of the lua file to load when initializing lua state
pub const MAIN_LUA_FILE: &str = "main.lua";

// the name of the thumbnail
pub const THUMBNAIL_FILE: &str = "thumbnail.png";

// the name of the game specifications file
pub const GAME_SPECS_FILE: &str = "gameinfo.json";

#[derive(Debug, Clone)]
pub struct GameEntry<T> {
    pub title: String,
    pub directory: PathBuf,
    pub thumbnail: Option<T>,
}

#[derive(Debug, Clone)]
pub enum GameEntryError {
    DirectoryCouldNotBeRead,
    LuaEntryPointCouldNotBeRead,
    SpecsCouldNotBeRead,
    SpecDeserializeError,
}

#[derive(Debug, Deserialize)]
pub struct GameSpecs {
    pub title: String,
}

fn is_readable(path: &Path) -> bool {
    match std::fs::exists(path) {
        Ok(exists) => exists,
        Err(_) => false,
    }
}

impl<T> GameEntry<T> {
    pub fn try_load<ITC: ITextureCompiler<Texture = T>>(
        directory: PathBuf,
        texture_compiler: &mut ITC,
    ) -> Result<Self, GameEntryError> {
        if !is_readable(&directory) {
            return Err(GameEntryError::DirectoryCouldNotBeRead);
        }

        let main_path = directory.join(MAIN_LUA_FILE);
        if !is_readable(&main_path) {
            return Err(GameEntryError::LuaEntryPointCouldNotBeRead);
        }

        let specs_path = directory.join(GAME_SPECS_FILE);
        let specs_file = match File::open(specs_path) {
            Ok(f) => f,
            Err(_) => return Err(GameEntryError::SpecsCouldNotBeRead),
        };
        let specs_reader = BufReader::new(specs_file);
        let specs: GameSpecs = match serde_json::from_reader(specs_reader) {
            Ok(specs) => specs,
            Err(_) => return Err(GameEntryError::SpecDeserializeError),
        };
        let title = specs.title;

        let thumbnail_path = directory.join(THUMBNAIL_FILE);
        let thumbnail = match texture_compiler.load_texture(&thumbnail_path) {
            Ok(t) => Some(t),
            Err(_) => None,
        };

        Ok(Self {
            thumbnail,
            title,
            directory,
        })
    }
}

pub fn load_game_entries<T, ITC: ITextureCompiler<Texture = T>>(
    games_directory: &Path,
    texture_compiler: &mut ITC,
) -> Vec<GameEntry<T>> {
    let mut entries = Vec::new();

    let dir_iter = if let Ok(i) = std::fs::read_dir(games_directory) {
        i
    } else {
        return entries;
    };

    for entry_result in dir_iter {
        let entry = if let Ok(e) = entry_result {
            e
        } else {
            continue;
        };
        let file_type = if let Ok(ft) = entry.file_type() {
            ft
        } else {
            continue;
        };

        if file_type.is_dir() {
            if let Ok(game_entry) = GameEntry::<T>::try_load(entry.path(), texture_compiler) {
                entries.push(game_entry);
            }
        }
    }

    entries
}

#[derive(Debug, Clone)]
pub struct GameEndHandle {
    end_game: Rc<Cell<bool>>,
}

impl GameEndHandle {
    pub fn end_game(&self) {
        self.end_game.set(false);
    }
}

#[derive(Debug, Clone)]
pub struct GameOverState {
    end_game: Rc<Cell<bool>>,
}

impl GameOverState {
    pub fn new() -> Self {
        Self {
            end_game: Rc::new(Cell::new(false)),
        }
    }

    pub fn end_game_handle(&self) -> GameEndHandle {
        GameEndHandle {
            end_game: self.end_game.clone(),
        }
    }

    pub fn is_over(&self) -> bool {
        self.end_game.get()
    }
}
