use std::{fmt::Debug, path::Path};

use crate::{
    assets, board::BoardManager, cheat_buffer::CheatBuffer, color::IColor, geometry::{IConvexShape, IRect, IVec2}, intermediate_representations::VisualInfo, lua::FromLuaTable, resources::{TextureInfo, VisualComponentProvider}
};

#[derive(Debug)]
pub struct GlobalAssets<T, F> {
    pub open_hand_texture: T,
    pub closed_hand_texture: T,
    pub cheat_buffer_font: F,
}

impl<T: 'static, F: 'static> GlobalAssets<T, F> {
    pub const CHEAT_BUFFER_FONT_SIZE: f32 = 28.0;

    pub fn new<R: ITextureCompiler<Texture = T, Font = F>>(compiler: &mut R) -> Self {
        let open_hand_texture = match compiler.load_texture_from_memory(assets::OPEN_HAND.filetype, assets::OPEN_HAND.data) {
            Ok(t) => t,
            Err(msg) => panic!("Unable to load open hand texture: {msg}"),
        };
        let closed_hand_texture = match compiler.load_texture_from_memory(assets::CLOSED_HAND.filetype, assets::CLOSED_HAND.data)
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
        visual: &VisualInfo<Self::Color, Self::Rectangle, Self::Vector2>,
        magnification_dimensions: Self::Vector2,
        visual_component_provider: VisualComponentProvider<'_, Self::Font, Self::Texture>,
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
