use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    path::{Path, PathBuf},
    str::FromStr,
};

use roaring::RoaringBitmap;

use crate::{
    ecs::{Entity, NamedEntityManager, ResourceManager},
    geometry::{CollisionShape, IConvexShape, IVec2},
    piece_components::TextureInfo,
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
    fonts: *mut FontManager<F>,

    /// the images used for rendering visuals
    images: *mut HashMap<String, I>,

    marker: PhantomData<&'a ()>,
}

impl<'a, F, I> VisualComponentProvider<'a, F, I> {
    pub fn new(fonts: &'a mut FontManager<F>, images: &'a mut HashMap<String, I>) -> Self {
        Self {
            fonts: fonts as *mut FontManager<F>,
            images: images as *mut HashMap<String, I>,
            marker: PhantomData,
        }
    }

    pub fn get_or_load_font<R: IRenderer<Font = F>>(
        &mut self,
        renderer: &mut R,
        path: &String,
        size: u32,
        characters: &str,
    ) -> Option<&F> {
        // SAFETY: only one font reference can be acquired at the same time because this struct is
        // borrowed mutably, there is no danger of multiple mutable references to the font manager
        let fonts = unsafe { &mut *self.fonts };
        if let Some(f) = fonts.get_font(path, size, characters) {
            return Some(f);
        }

        let fonts = unsafe { &mut *self.fonts };
        fonts.load_font(renderer, path, size, characters);
        fonts.get_font(path, size, characters)
    }

    pub fn get_or_load_image<R: IRenderer<Image = I>>(
        &mut self,
        renderer: &mut R,
        path: &String,
    ) -> Option<&I> {
        // SAFETY: only one image reference can be acquired at the same time because this struct is
        // borrowed mutably, there is no danger of multiple mutable references to the image manager
        let images = unsafe { &mut *self.images };
        if let Some(i) = images.get(path) {
            return Some(i);
        }

        let images = unsafe { &mut *self.images };
        let path_buf = PathBuf::from(path);
        let image = if let Ok(i) = renderer.load_image(&path_buf) {
            i
        } else {
            return None;
        };
        images.insert(path.clone(), image);
        images.get(path)
    }
}

/// an entry in a font manager for fonts saved to memory
pub struct FontEntry<F> {
    /// the set of characters loaded for this font
    pub charset: RoaringBitmap,

    /// a map from font size to font
    pub sizes: BTreeMap<u32, F>,
}

pub struct FontManager<F> {
    fonts: HashMap<String, FontEntry<F>>,
}

impl<F> Default for FontManager<F> {
    fn default() -> Self {
        Self {
            fonts: Default::default(),
        }
    }
}

impl<F> FontManager<F> {
    pub fn new() -> Self {
        Self::default()
    }

    /// loads a font, returning true on success or if the font was already loaded
    pub fn load_font<R: IRenderer<Font = F>>(
        &mut self,
        renderer: &mut R,
        path: &String,
        size: u32,
        characters: &str,
    ) -> bool {
        let path_result = PathBuf::try_from(path);
        if path_result.is_err() {
            return false;
        }
        let path_buf = path_result.unwrap();

        if let Some(entry) = self.fonts.get_mut(path) {
            // update existing entry with new characters
            let mut needs_update = false;
            for c in characters.chars() {
                if entry.charset.insert(c as u32) {
                    needs_update = true;
                }
            }

            if !needs_update && entry.sizes.contains_key(&size) {
                return true;
            }

            let characters: String = entry
                .charset
                .iter()
                .map(|e| unsafe { char::from_u32_unchecked(e) })
                .collect();
            if needs_update {
                // reload all contained font sizes
                for (size, font) in entry.sizes.iter_mut() {
                    let font_result = renderer.load_font(&path_buf, *size, &characters);
                    if font_result.is_err() {
                        return false;
                    }
                    *font = font_result.unwrap();
                }
            }

            // insert the new font size
            let font_result = renderer.load_font(&path_buf, size, &characters);
            if font_result.is_err() {
                return false;
            }
            let font = font_result.unwrap();
            entry.sizes.insert(size, font);

            return true;
        }

        // completely new font entry
        let font_result = renderer.load_font(&path_buf, size, characters);
        if font_result.is_err() {
            return false;
        }
        let font = font_result.unwrap();

        let entry = FontEntry {
            charset: RoaringBitmap::from_iter(characters.chars().map(|c| c as u32)),
            sizes: BTreeMap::from([(size, font)]),
        };
        self.fonts.insert(path.clone(), entry);

        true
    }

    pub fn get_font(&self, path: &String, size: u32, characters: &str) -> Option<&F> {
        let font_entry = self.fonts.get(path)?;
        if characters
            .chars()
            .position(|c| !font_entry.charset.contains(c as u32))
            .is_some()
        {
            None
        } else {
            font_entry.sizes.get(&size)
        }
    }
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
pub struct GameResources<V: IVec2, S: IConvexShape<V>, T, F, I> {
    /// loaded fonts to speed up font rendering capabilities
    pub fonts: FontManager<F>,

    /// loaded images to speed up image texture generation capabilities
    pub images: HashMap<String, I>,

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

impl<V: IVec2, S: IConvexShape<V>, T, F, I> Default for GameResources<V, S, T, F, I> {
    fn default() -> Self {
        Self {
            fonts: Default::default(),
            images: Default::default(),
            piece_textures: Default::default(),
            collision_shapes: Default::default(),
            collision_layers: Default::default(),
            draw_layers: Default::default(),
            piece_categories: Default::default(),
        }
    }
}
