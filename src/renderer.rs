use std::path::Path;

use crate::{
    color::IColor,
    geometry::{IRect, IVec2},
    piece_components::{TextureInfo, VisualInfo}, resources::VisualComponentProvider,
};

pub trait IRenderer {
    type Texture;
    type Font;

    /// compile a visual into a to-scale and magnified texture, assuming the given magnification dimensions
    /// image_provider maps a name to an image, all image names provided in the visual are valid,
    /// but may panic if a name not in the provided visual info is used
    fn compile_visual<C, R, V>(
        &mut self,
        visual: &VisualInfo<C, R, V>,
        magnification_dimensions: V,
        visual_component_provider: VisualComponentProvider<'_, Self::Font, Self::Texture>,
    ) -> Result<TextureInfo<Self::Texture>, String>
    where
        C: IColor,
        R: IRect,
        V: IVec2;


    /// loads a font into an acceptable format, given a path to its font file
    /// return a result with a message for an error
    fn load_font(&mut self, path: &Path, size: u32, characters: &str) -> Result<Self::Font, String>;

    /// loads an image into an acceptable format
    /// return a result with a message for an error
    fn load_image(&mut self, path: &Path) -> Result<Self::Texture, String>;
}
