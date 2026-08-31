use std::{collections::{BTreeMap, HashMap, HashSet}, path::{Path, PathBuf}};

use roaring::RoaringBitmap;

use crate::{
    color::IColor,
    ecs::{Entity, NamedEntityManager, ResourceManager},
    geometry::{CollisionShape, IConvexShape, IRect, IVec2},
    piece_components::{
        ImageInfo, PatternInfo, TextboxInfo, TextureInfo, VisualComponent, VisualInfo,
    },
    renderer::IRenderer,
};

pub struct RuntimeInfo<'a, R: IRenderer> {
    pub project_directory: &'a Path,
    pub renderer: &'a mut R,
}

impl<'a, R: IRenderer> RuntimeInfo<'a, R> {
    #[cfg(target_os = "windows")]
    /// gets all possible font paths provided in order the that they should be searched, including:
    ///  - Project-local fonts
    ///  - User fonts
    ///  - System fonts
    pub fn font_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        paths.push(self.project_directory.join("fonts"));
        if let Some(home) = std::env::home_dir() {
            paths.push(home.join(r"AppData\Local\Microsoft\Windows\Fonts"))
        }
        paths.push(PathBuf::from(r"C:\Windows\Fonts"));

        paths
    }

    #[cfg(target_os = "linux")]
    /// gets all possible font paths provided in order the that they should be searched, including:
    ///  - System fonts
    ///  - User fonts
    ///  - Project-local fonts
    pub fn font_paths(&self) -> PathBuf {
        todo!()
    }

    #[cfg(target_os = "macos")]
    /// gets all possible font paths provided in order the that they should be searched, including:
    ///  - System fonts
    ///  - User fonts
    ///  - Project-local fonts
    pub fn font_paths(&self) -> PathBuf {
        todo!()
    }

    pub fn image_path(&self) -> PathBuf {
        let path = self.project_directory;
        path.join("images")
    }
}

pub struct VisualComponentProvider<'a, F, I> {
    /// fonts usable in rendering visuals
    fonts: &'a HashMap<String, FontEntry<F>>,

    /// the images used for rendering visuals
    images: &'a HashMap<String, I>,
}

impl<'a, F, I> Clone for VisualComponentProvider<'a, F, I> {
    fn clone(&self) -> Self {
        Self {
            fonts: self.fonts,
            images: self.images,
        }
    }
}
impl<'a, F, I> Copy for VisualComponentProvider<'a, F, I> {}

impl<'a, F, I> VisualComponentProvider<'a, F, I> {
    pub fn get_font(&self, name: &String) -> &F {
        self.fonts
            .get_by_name(name)
            .expect(&format!("Failed to get font '{}'; not loaded.", name))
    }

    pub fn get_image(&self, name: &String) -> &I {
        self.images
            .get_by_name(name)
            .expect(&format!("Failed to get image '{}'; not loaded.", name))
    }
}

/// a manager for fonts saved to memory
pub struct FontEntry<F> {
    /// the set of characters loaded for this font
    pub charset: RoaringBitmap,

    /// a map from font size to font
    pub sizes: BTreeMap<u32, F>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureEntityMarker;
pub type TextureEntity = Entity<TextureEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionShapeEntityMarker;
pub type CollisionShapeEntity = Entity<CollisionShapeEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CollisionLayerEntityMarker;
pub type CollisionLayerEntity = Entity<CollisionLayerEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrawLayerEntityMarker;
pub type DrawLayerEntity = Entity<DrawLayerEntityMarker>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PieceCategoryEntityMarker;
pub type PieceCategoryEntity = Entity<PieceCategoryEntityMarker>;

/// persistent global game components
pub struct GameResources<C: IColor, R: IRect, V: IVec2, S: IConvexShape<V>, T, F, I> {
    /// loaded fonts to speed up font rendering capabilities
    pub fonts: 

    /// loaded images to speed up image texture generation capabilities
    pub images: HashMap<String, F>,

    /// textures rendered from piece visuals
    pub piece_textures: ResourceManager<TextureEntity, TextureInfo<T>>,

    /// collision shape resources that may be combined for use on piece faces
    pub collision_shapes: ResourceManager<CollisionLayerEntity, CollisionShape<S, V>>,

    /// the names of the collision layers for this game
    pub collision_layers: NamedEntityManager<CollisionLayerEntity>,

    /// the names of the draw layers for this game
    pub draw_layers: NamedEntityManager<DrawLayerEntity>,

    /// the special piece categorizations for this game
    pub piece_categories: NamedEntityManager<PieceCategoryEntity>,
}

impl<C: IColor, R: IRect, V: IVec2, S: IConvexShape<V>, T, F, I> Default
    for GameResources<C, R, V, S, T, F, I>
{
    fn default() -> Self {
        Self {
            visuals: Default::default(),
            piece_textures: Default::default(),
            collision_shapes: Default::default(),
            collision_layers: Default::default(),
            draw_layers: Default::default(),
            piece_categories: Default::default(),
        }
    }
}
