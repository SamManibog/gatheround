use std::path::Path;

use crate::{
    board::BoardManager,
    color::IColor,
    geometry::{IConvexShape, IRect, IVec2},
    intermediate_representations::VisualInfo,
    lua::FromLuaTable,
    resources::{TextureInfo, VisualComponentProvider},
};

#[derive(Debug)]
pub struct GlobalAssets<T> {
    pub open_hand_texture: T,
    pub closed_hand_texture: T,
}

const OPEN_HAND_BYTES: &[u8] = include_bytes!("open_hand.png");
const CLOSED_HAND_BYTES: &[u8] = include_bytes!("closed_hand.png");

impl<T> GlobalAssets<T> {
    pub fn new<R: ITextureCompiler<Texture = T>>(compiler: &mut R) -> Self {
        let open_hand_texture = match compiler.load_texture_from_memory(".png", OPEN_HAND_BYTES) {
            Ok(t) => t,
            Err(msg) => panic!("Unable to load open hand texture: {msg}"),
        };
        let closed_hand_texture = match compiler.load_texture_from_memory(".png", CLOSED_HAND_BYTES)
        {
            Ok(t) => t,
            Err(msg) => panic!("Unable to load open hand texture: {msg}"),
        };

        Self {
            open_hand_texture,
            closed_hand_texture,
        }
    }
}

pub trait ITextureCompiler {
    type Texture;
    type Font;
    type Color: IColor;
    type Rectangle: IRect;
    type Vector2: IVec2;

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
    type Texture;
    type Font;
    type Color: IColor;
    type Rectangle: IRect;
    type Vector2: IVec2;
    type ConvexShape: IConvexShape + FromLuaTable;

    /// draws a board to a texture
    fn draw_board(
        &mut self,
        board: &BoardManager<
            Self::Color,
            Self::Rectangle,
            Self::Vector2,
            Self::ConvexShape,
            Self::Texture,
            Self::Font,
        >,
        assets: &GlobalAssets<Self::Texture>,
        output_texture: &mut Self::Texture,
    );
}
