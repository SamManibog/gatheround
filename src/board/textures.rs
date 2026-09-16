#[cfg(target_os = "windows")]
use std::path::PathBuf;
use std::{
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    path::Path,
};

use roaring::RoaringBitmap;

use crate::{
    assets::{self, BuiltinFontData}, board::irs::TextureIR, platform::ITextureCompiler, utils::{
        color::IColor,
        ecs::{Entity, ResourceManager},
        geometry::{IRect, IVec2},
    }
};

#[cfg(target_os = "windows")]
/// gets all possible font paths provided in order the that they should be searched, including:
///  - Project-local fonts
///  - User fonts
///  - System fonts
pub fn get_font_paths(project_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    paths.push(project_path.to_path_buf());
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

/// texture info for a piece face
#[derive(Debug, Clone)]
pub struct TextureInfo<T> {
    /// the texture used to draw this piece
    pub texture: T,

    /// an enlarged version of this texture for zoomed views
    pub texture_magnified: T,
}

pub struct TextureComponentProvider<'a, F, I> {
    /// fonts usable in rendering visuals
    fonts: *mut FontManager<F>,

    /// the images used for rendering visuals
    images: *mut HashMap<String, I>,

    /// the project's directory
    project_path: &'a Path,

    marker: PhantomData<&'a ()>,
}

impl<'a, F, I> TextureComponentProvider<'a, F, I> {
    pub fn new(
        project_path: &'a Path,
        fonts: &'a mut FontManager<F>,
        images: &'a mut HashMap<String, I>,
    ) -> Self {
        Self {
            project_path,
            fonts: fonts as *mut FontManager<F>,
            images: images as *mut HashMap<String, I>,
            marker: PhantomData,
        }
    }

    pub fn get_or_load_font<R: ITextureCompiler<Font = F>>(
        &mut self,
        renderer: &mut R,
        font_name: &String,
        size: u32,
        characters: &str,
    ) -> Option<&F> {
        // SAFETY: only one font reference can be acquired at the same time because this struct is
        // borrowed mutably, there is no danger of multiple mutable references to the font manager
        let fonts = unsafe { &mut *self.fonts };
        if let Some(f) = fonts.get_font(font_name, size, characters) {
            return Some(f);
        }

        let fonts = unsafe { &mut *self.fonts };
        fonts.load_font(renderer, self.project_path, font_name, size, characters);
        fonts.get_font(font_name, size, characters)
    }

    pub fn get_or_load_image<R: ITextureCompiler<Texture = I>>(
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
        let path_buf = self.project_path.join(path);
        let image = if let Ok(i) = renderer.load_texture(&path_buf) {
            i
        } else {
            return None;
        };
        images.insert(path.clone(), image);
        images.get(path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureEntityMarker;
pub type TextureEntity = Entity<TextureEntityMarker>;

/// texture resources
#[derive(Debug)]
pub struct TextureManager<T, F> {
    /// loaded fonts to speed up font rendering capabilities
    fonts: FontManager<F>,

    /// loaded images to speed up image texture generation capabilities
    images: HashMap<String, T>,

    /// textures
    textures: ResourceManager<TextureEntity, TextureInfo<T>>,
}

impl<T, F> Default for TextureManager<T, F> {
    fn default() -> Self {
        Self {
            fonts: Default::default(),
            images: Default::default(),
            textures: Default::default(),
        }
    }
}

impl<T, F> TextureManager<T, F> {
    /// adds a new image, overwritting it if it already exists
    pub fn add_image(&mut self, name: String, image: T) {
        self.images.insert(name, image);
    }

    /// adds a new texture
    pub fn add_texture(
        &mut self,
        name: Option<String>,
        texture: TextureInfo<T>,
    ) -> Result<TextureEntity, String> {
        if let Some(n) = name {
            if let Ok(entity) = self.textures.add_named(n.clone(), texture) {
                Ok(entity)
            } else {
                Err(format!("Texture with name '{n}' already exists."))
            }
        } else {
            Ok(self.textures.add(texture))
        }
    }

    /// attempts to create a new texture, returning its entity on success
    /// textures may be assigned an optional name that can be used to identify it
    pub fn load_texture<C, R, V, IR>(
        &mut self,
        project_path: &Path,
        name: Option<String>,
        visual: &TextureIR<C, R, V>,
        magnification_dimensions: V,
        renderer: &mut IR,
    ) -> Result<TextureEntity, String>
    where
        C: IColor,
        R: IRect,
        V: IVec2,
        IR: ITextureCompiler<Font = F, Texture = T, Color = C, Rectangle = R, Vector2 = V>,
    {
        let provider = TextureComponentProvider {
            project_path,
            fonts: &mut self.fonts,
            images: &mut self.images,
            marker: PhantomData,
        };

        let texture = renderer.compile_visual(visual, magnification_dimensions, provider)?;
        self.add_texture(name, texture)
    }

    /// removes a texture if it exists, returning it and its name
    pub fn remove_texture(
        &mut self,
        entity: TextureEntity,
    ) -> Option<(Option<String>, TextureInfo<T>)> {
        self.textures.remove(entity)
    }

    /// removes a texture by its name if it exists
    pub fn remove_texture_by_name(&mut self, name: &String) -> Option<TextureInfo<T>> {
        self.textures.remove_by_name(name)
    }

    /// checks if a texture with the given entity exists
    pub fn contains_texture(&self, entity: TextureEntity) -> bool {
        self.textures.contains(entity)
    }

    /// checks if a texture with the given name exists
    pub fn contains_texture_name(&self, name: &String) -> bool {
        self.textures.contains_name(name)
    }

    /// gets a texture by its entity
    pub fn get_texture(&self, entity: TextureEntity) -> Option<&TextureInfo<T>> {
        self.textures.get(entity)
    }

    /// gets a mutable texture by its entity
    pub fn get_texture_mut(&mut self, entity: TextureEntity) -> Option<&mut TextureInfo<T>> {
        self.textures.get_mut(entity)
    }

    /// gets a reference to the texture by name
    pub fn get_texture_by_name(&self, name: &String) -> Option<&TextureInfo<T>> {
        self.textures.get_by_name(name)
    }

    /// gets a mutable reference to the texture by name
    pub fn get_texture_mut_by_name(&mut self, name: &String) -> Option<&mut TextureInfo<T>> {
        self.textures.get_mut_by_name(name)
    }

    /// gets the name of the entity
    pub fn get_texture_name(&self, entity: TextureEntity) -> Option<&String> {
        self.textures.get_name(entity)
    }

    /// gets the entity by the given name
    pub fn get_texture_entity(&self, name: &String) -> Option<TextureEntity> {
        self.textures.get_entity(name)
    }

    /// returns an iterator over all entities, names, and textures
    pub fn iter(&self) -> impl Iterator<Item = (TextureEntity, Option<&String>, &TextureInfo<T>)> {
        self.textures.iter()
    }

    /// returns an iterator over entities and their names
    pub fn identifier_iter(&self) -> impl Iterator<Item = (TextureEntity, Option<&String>)> {
        self.textures.identifier_iter()
    }

    /// returns an iterator over all textures
    pub fn texture_iter(&self) -> impl Iterator<Item = &TextureInfo<T>> {
        self.textures.resource_iter()
    }

    /// returns a mutable iterator over all textures
    pub fn texture_iter_mut(&mut self) -> impl Iterator<Item = &mut TextureInfo<T>> {
        self.textures.resource_iter_mut()
    }

    /// loads a font, returning true on successful load
    pub fn load_font<R: ITextureCompiler<Font = F>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        path: &String,
        sizes: &[u32],
        characters: &str,
    ) -> bool {
        self.fonts
            .batch_load_font(renderer, project_path, path, sizes, characters)
    }

    /// checks if a font with the given pathis loaded
    pub fn is_font_loaded(&self, path: &String, size: u32, characters: &str) -> bool {
        self.fonts.get_font(path, size, characters).is_some()
    }

    /// clears the cached fonts
    pub fn clear_font_cache(&mut self) {
        self.fonts.clear();
    }

    /// loads an image, returning true on successful load
    pub fn load_image<R: ITextureCompiler<Texture = T>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        path: &String,
    ) -> bool {
        let path_buf = project_path.join(path);
        let image = if let Ok(i) = renderer.load_texture(&path_buf) {
            i
        } else {
            return false;
        };

        self.images.insert(path.clone(), image);
        true
    }

    /// checks if an image is loaded
    pub fn is_image_loaded(&self, path: &String) -> bool {
        self.images.contains_key(path)
    }

    /// clears the cached images
    pub fn clear_image_cache(&mut self) {
        self.images.clear();
    }

    /// clears all data from the manager
    pub fn clear(&mut self) {
        self.fonts.clear();
        self.images.clear();
        self.textures.clear();
    }
}

/// an entry in a font manager for fonts saved to memory
#[derive(Debug)]
pub struct FontEntry<F> {
    /// the set of characters loaded for this font
    pub charset: RoaringBitmap,

    /// a map from font size to font
    pub sizes: BTreeMap<u32, F>,
}

#[derive(Debug)]
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

enum FontLoadIdentifier<'a> {
    Path(&'a Path),
    Builtin(&'static BuiltinFontData),
}

impl<F> FontManager<F> {
    pub fn new() -> Self {
        Self::default()
    }

    // remvoes all fonts
    pub fn clear(&mut self) {
        self.fonts.clear();
    }

    /// batch loads multiple font sizes at once
    /// returns true if all sizes could be loaded
    pub fn batch_load_font<R: ITextureCompiler<Font = F>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        font_name: &String,
        sizes: &[u32],
        characters: &str,
    ) -> bool {
        'path_loop: for font_dir in get_font_paths(project_path) {
            let path_buf = font_dir.join(font_name);

            match std::fs::exists(&path_buf) {
                Ok(true) => (),
                Ok(false) => continue 'path_loop,
                Err(_) => continue 'path_loop,
            }

            if self.batch_load_font_helper(
                renderer,
                FontLoadIdentifier::Path(&path_buf),
                font_name,
                sizes,
                characters,
            ) {
                return true;
            }
        }

        if let Some(builtin) = BuiltinFontData::get_by_name(&font_name) {
            if self.batch_load_font_helper(
                renderer,
                FontLoadIdentifier::Builtin(&builtin),
                font_name,
                sizes,
                characters,
            ) {
                return true;
            }
        }

        false
    }

    /// batch loads multiple font sizes at once
    /// returns true if all sizes could be loaded
    fn batch_load_font_helper<R: ITextureCompiler<Font = F>>(
        &mut self,
        renderer: &mut R,
        font_identifier: FontLoadIdentifier,
        font_name: &String,
        sizes: &[u32],
        characters: &str,
    ) -> bool {
        if let Some(entry) = self.fonts.get_mut(font_name) {
            // update existing entry with new characters
            let mut needs_update = false;
            for c in characters.chars() {
                if entry.charset.insert(c as u32) {
                    needs_update = true;
                }
            }

            let characters: String = entry
                .charset
                .iter()
                .map(|e| unsafe { char::from_u32_unchecked(e) })
                .collect();
            if needs_update {
                // reload all contained font sizes
                for (size, font) in entry.sizes.iter_mut() {
                    let font_result = match font_identifier {
                        FontLoadIdentifier::Path(path) => {
                            renderer.load_font(&path, *size, &characters)
                        }
                        FontLoadIdentifier::Builtin(builtin) => renderer.load_font_from_memory(
                            builtin.filetype,
                            builtin.data,
                            *size,
                            &characters,
                        ),
                    };
                    if font_result.is_err() {
                        return false;
                    }
                    *font = font_result.unwrap();
                }
            }

            // insert the new font size
            for size in sizes {
                if entry.sizes.contains_key(size) {
                    continue;
                }

                let font_result = match font_identifier {
                    FontLoadIdentifier::Path(path) => renderer.load_font(&path, *size, &characters),
                    FontLoadIdentifier::Builtin(builtin) => renderer.load_font_from_memory(
                        builtin.filetype,
                        builtin.data,
                        *size,
                        &characters,
                    ),
                };
                if font_result.is_err() {
                    return false;
                }
                let font = font_result.unwrap();
                entry.sizes.insert(*size, font);
            }

            return true;
        }

        // completely new font entry
        let mut temp_map = BTreeMap::new();
        let characters = format!("{characters}{}", assets::ASCII_GRAPHIC_CHARSET);
        for size in sizes {
            let font_result = match font_identifier {
                FontLoadIdentifier::Path(path) => renderer.load_font(&path, *size, &characters),
                FontLoadIdentifier::Builtin(builtin) => renderer.load_font_from_memory(
                    builtin.filetype,
                    builtin.data,
                    *size,
                    &characters,
                ),
            };
            if font_result.is_err() {
                if !temp_map.is_empty() {
                    let entry = FontEntry {
                        charset: RoaringBitmap::from_iter(
                            characters
                                .chars()
                                .chain(assets::ASCII_GRAPHIC_CHARSET.chars())
                                .map(|c| c as u32),
                        ),
                        sizes: temp_map,
                    };
                    self.fonts.insert(font_name.clone(), entry);
                }

                return false;
            }
            temp_map.insert(*size, font_result.unwrap());
        }

        let entry = FontEntry {
            charset: RoaringBitmap::from_iter(characters.chars().map(|c| c as u32)),
            sizes: temp_map,
        };
        self.fonts.insert(font_name.clone(), entry);

        true
    }

    /// loads a single font, returning true on success or if the font was already loaded
    pub fn load_font<R: ITextureCompiler<Font = F>>(
        &mut self,
        renderer: &mut R,
        project_path: &Path,
        font_name: &String,
        size: u32,
        characters: &str,
    ) -> bool {
        let sizes = [size; 1];
        self.batch_load_font(renderer, project_path, font_name, &sizes, characters)
    }

    pub fn get_font(&self, font_name: &String, size: u32, characters: &str) -> Option<&F> {
        let font_entry = self.fonts.get(font_name)?;
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
