use std::{fmt::Debug, path::Path};

use crate::{
    app::GlobalAssets,
    board::{
        BoardManager,
        irs::TextureIR,
        textures::{TextureComponentProvider, TextureInfo},
    },
    cheat_buffer::CheatBuffer,
    lua::FromLuaTable,
    utils::{
        color::IColor,
        geometry::{IConvexShape, IRect, IVec2},
    },
};

pub mod raylib;

pub trait ITextureCompiler {
    type Texture: 'static;
    type Font: 'static;
    type Color: IColor + 'static;
    type Rectangle: IRect + 'static;
    type Vector2: IVec2 + 'static;

    /// compile a visual into a to-scale and magnified texture, assuming the given magnification dimensions
    /// image_provider maps a name to an image, all image names provided in the visual are valid,
    /// but may panic if a name not in the provided visual info is used
    fn compile_visual(
        &mut self,
        visual: &TextureIR<Self::Color, Self::Rectangle, Self::Vector2>,
        magnification_dimensions: Self::Vector2,
        visual_component_provider: TextureComponentProvider<'_, Self::Font, Self::Texture>,
    ) -> Result<TextureInfo<Self::Texture>, String>;

    /// loads a font into an acceptable format, given a path to its font file
    /// return a result with a message for an error
    fn load_font(&mut self, path: &Path, size: u32, characters: &str)
    -> Result<Self::Font, String>;

    /// loads a font from memory into an acceptable format
    /// return a result with a message for an error
    fn load_font_from_memory(
        &mut self,
        filetype: &str,
        bytes: &[u8],
        size: u32,
        characters: &str,
    ) -> Result<Self::Font, String>;

    /// loads an image into an acceptable format
    /// return a result with a message for an error
    fn load_texture(&mut self, path: &Path) -> Result<Self::Texture, String>;

    /// loads an image from memory into an acceptable format
    /// return a result with a message for an error
    fn load_texture_from_memory(
        &mut self,
        filetype: &str,
        bytes: &[u8],
    ) -> Result<Self::Texture, String>;
}

pub trait IRenderer {
    type Texture: 'static;
    type Font: 'static;
    type Color: IColor + 'static;
    type Rectangle: IRect + 'static;
    type Vector2: IVec2 + 'static;
    type ConvexShape: IConvexShape + FromLuaTable + Debug + From<Self::Rectangle> + 'static;

    /// draws the app
    fn draw_app(
        &mut self,
        board: &BoardManager<
            Self::Color,
            Self::Rectangle,
            Self::Vector2,
            Self::ConvexShape,
            Self::Texture,
            Self::Font,
        >,
        cheat_buffer: &CheatBuffer,
        assets: &GlobalAssets<Self::Texture, Self::Font>,
    );
}

pub trait IKeyboardReader {
    /// check if the up key is currently pressed (need not be on this frame)
    fn is_up_key_pressed(&mut self) -> bool;

    /// check if the down key is currently pressed (need not be on this frame)
    fn is_down_key_pressed(&mut self) -> bool;

    /// check if the left key is currently pressed (need not be on this frame)
    fn is_left_key_pressed(&mut self) -> bool;

    /// check if the right key is currently pressed (need not be on this frame)
    fn is_right_key_pressed(&mut self) -> bool;

    /// check if the backspace key is currently pressed (need not be on this frame)
    fn is_backspace_key_pressed(&mut self) -> bool;

    /// check if the delete key is currently pressed (need not be on this frame)
    fn is_delete_key_pressed(&mut self) -> bool;

    /// check if the enter key is currently pressed (need not be on this frame)
    fn is_enter_key_pressed(&mut self) -> bool;

    /// poll for pressed characters
    fn poll_char_press(&mut self) -> Option<char>;
}

pub trait IPlatform: 'static {
    type Texture: 'static;
    type Font: 'static;
    type Color: IColor + 'static;
    type Rectangle: IRect + 'static;
    type Vector2: IVec2 + 'static;
    type ConvexShape: IConvexShape + FromLuaTable + Debug + From<Self::Rectangle> + 'static;

    type KeyboardReader<'a>: IKeyboardReader;

    type Renderer<'a>: IRenderer<
            Texture = Self::Texture,
            Font = Self::Font,
            Color = Self::Color,
            Rectangle = Self::Rectangle,
            Vector2 = Self::Vector2,
            ConvexShape = Self::ConvexShape,
        >;

    type TextureCompiler<'a>: ITextureCompiler<
            Texture = Self::Texture,
            Font = Self::Font,
            Color = Self::Color,
            Rectangle = Self::Rectangle,
            Vector2 = Self::Vector2,
        >;

    /// if the platform requests a close
    fn should_close(&mut self) -> bool;

    /// the time between frames
    /// this value shouldn't change
    fn delta_time(&mut self) -> f32;

    /// gets the platform's renderer
    fn renderer(&mut self) -> Self::Renderer<'_>;

    /// gets the platform's texture compiler
    fn texture_compiler(&mut self) -> Self::TextureCompiler<'_>;

    /// gets the platforms keyboard reader
    fn keyboard_reader(&mut self) -> Self::KeyboardReader<'_>;
}
