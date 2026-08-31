use mlua::Table;
use regex::Regex;

use crate::{
    color::IColor,
    geometry::{IRect, IVec2},
};

/// a transform for an image, only covers offset and scale
/// (scale is applied before offset)
#[derive(Debug, Clone)]
pub struct ImageTransform<V: IVec2> {
    pub offset: V,
    pub scale: f32,
}

impl<V: IVec2> ImageTransform<V> {
    pub fn identity() -> Self {
        Self {
            offset: V::from_xy(0.0, 0.0),
            scale: 1.0,
        }
    }

    pub fn transform_vector<T1: IVec2, T2: IVec2>(&self, vector: T1) -> T2 {
        T2::from_other(vector.scaled(self.scale).add(self.offset))
    }

    pub fn transform_rect<R1: IRect, R2: IRect>(&self, rect: R1) -> R2 {
        let min: (f32, f32) = self.transform_vector(rect.min::<(f32, f32)>());
        let max: (f32, f32) = self.transform_vector(rect.max::<(f32, f32)>());

        R2::from_min_max(min, max)
    }

    pub fn transform_other<V1: IVec2, V2: IVec2>(
        &self,
        other: &ImageTransform<V1>,
    ) -> ImageTransform<V2> {
        ImageTransform::<V2> {
            offset: self.transform_vector(other.offset),
            scale: self.scale * other.scale,
        }
    }
}

/// information required to put an image on a piece
#[derive(Debug, Clone)]
pub struct ImageInfo<V: IVec2> {
    pub transform: ImageTransform<V>,
    pub path: String,
}

impl<V: IVec2> ImageInfo<V> {
    pub fn try_from_lua_table(table: Table) -> Result<Self, String> {
        todo!()
    }
}

/// general alignment type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Begin,
    Center,
    End,
}

/// alignment text in a textbox
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    CenterCenter,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl TextAlign {
    pub fn vertical_align(&self) -> Alignment {
        match self {
            Self::TopLeft | Self::TopCenter | Self::TopRight => Alignment::Begin,
            Self::CenterLeft | Self::CenterCenter | Self::CenterRight => Alignment::Center,
            Self::BottomLeft | Self::BottomCenter | Self::BottomRight => Alignment::End,
        }
    }

    pub fn horizontal_align(&self) -> Alignment {
        match self {
            Self::TopLeft | Self::CenterLeft | Self::BottomLeft => Alignment::Begin,
            Self::TopCenter | Self::CenterCenter | Self::BottomCenter => Alignment::Center,
            Self::TopRight | Self::CenterRight | Self::BottomRight => Alignment::End,
        }
    }
}

/// information required to draw a text box on a piece
/// note that text wrapping is not yet supported
#[derive(Debug, Clone)]
pub struct TextboxInfo<C: IColor, R: IRect> {
    /// the space the text occupies
    pub rect: R,

    /// the contained text
    pub text: String,

    /// alignment of the contained text
    pub align: TextAlign,

    /// the font's size, None for automatic
    pub font_size: Option<u32>,

    /// the font's line spacing as a percent of font height
    /// None for automatic
    pub line_spacing: Option<f32>,

    /// the font's identifier, maybe a path or name
    pub font_family: String,

    /// the font's color
    pub color: C,
}

impl<C: IColor, R: IRect> TextboxInfo<C, R> {
    pub fn try_from_lua_table(table: Table) -> Result<Self, String> {
        todo!()
    }
}

/// a type of pattern
#[derive(Debug, Clone)]
pub enum PatternType {
    Solid,
    Checkered,
    Striped,
    Dotted,
}

/// information for a pattern
#[derive(Debug, Clone)]
pub struct PatternInfo<C: IColor, R: IRect, V: IVec2> {
    /// the rectangle in which the pattern should be drawn
    pub rect: R,

    /// the type of pattern to draw
    pub pattern: PatternType,

    /// the size of pattern elements
    pub weight: f32,

    /// the spacing between pattern elements
    pub spacing: f32,

    /// the angle of pattern elements
    pub angle: f32,

    /// the offset of pattern elements
    /// elements are anchored to the center of the rect
    pub offset: V,

    /// the color of pattern elements
    pub pattern_color: C,

    /// the color of the background
    pub background_color: C,
}

impl<C: IColor, R: IRect, V: IVec2> PatternInfo<C, R, V> {
    pub fn try_from_lua_table(table: Table) -> Result<Self, String> {
        todo!()
    }
}

/// a visual element on a piece face
#[derive(Debug, Clone)]
pub enum VisualComponent<C: IColor, R: IRect, V: IVec2> {
    Textbox(Box<TextboxInfo<C, R>>),
    Image(Box<ImageInfo<V>>),
    Pattern(Box<PatternInfo<C, R, V>>),
}

impl<C: IColor, R: IRect, V: IVec2> VisualComponent<C, R, V> {
    pub fn try_from_lua_table(table: Table) -> Result<Self, String> {
        let component = match table.get::<String>("type") {
            Ok(t) => {
                if t == "textbox" {
                    Self::Textbox(Box::new(TextboxInfo::try_from_lua_table(table)?))
                } else if t == "image" {
                    Self::Image(Box::new(ImageInfo::try_from_lua_table(table)?))
                } else if t == "pattern" {
                    Self::Pattern(Box::new(PatternInfo::try_from_lua_table(table)?))
                } else {
                    return Err(format!(
                        "Component has invalid type name '{t}', must be 'textbox', 'image', or 'pattern'."
                    ));
                }
            }

            Err(_) => {
                return Err(format!("Component has invalid type."));
            }
        };

        Ok(component)
    }
}

/// the render mode for the visual
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Nearest,
    Bilinear,
}

/// information for a visual element on a piece face
#[derive(Debug, Clone)]
pub struct VisualInfo<C: IColor, R: IRect, V: IVec2> {
    /// the assigned of each visual component
    names: Vec<String>,

    /// the visual components for the piece, in order from bottom to top
    components: Vec<VisualComponent<C, R, V>>,
}

impl<C: IColor, R: IRect, V: IVec2> Default for VisualInfo<C, R, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: IColor, R: IRect, V: IVec2> VisualInfo<C, R, V> {
    pub fn new() -> Self {
        Self {
            names: Default::default(),
            components: Default::default(),
        }
    }

    /// an iterator over all font names referenced in this visual
    pub fn font_iter(&self) -> impl Iterator<Item = &String> {
        self.components.iter().filter_map(|c| match c {
            VisualComponent::Textbox(t) => Some(&t.font_family),
            _ => None,
        })
    }

    /// an iterator over all images referenced in this visual
    pub fn image_iter(&self) -> impl Iterator<Item = &String> {
        self.components.iter().filter_map(|c| match c {
            VisualComponent::Image(i) => Some(&i.path),
            _ => None,
        })
    }

    /// gets a generic name that is valid to insert into this struct
    /// (starts at 1 for non-technical audiences (also lua indexing begins at one anyway))
    pub fn get_generic_name(&self, component: &VisualComponent<C, R, V>) -> String {
        // pigeonholes corresponding to each associated number, if true, then its not occupied
        let mut pigeonholes = vec![true; self.names.len() + 1];

        let regex = Regex::new(match component {
            VisualComponent::Textbox(_) => r"Textbox\s*\d+$",
            VisualComponent::Image(_) => r"Image\s*\d+$",
            VisualComponent::Pattern(_) => r"Pattern\s*\d+$",
        })
        .unwrap();

        // fill pigeon holes
        for name in self.names() {
            if regex.is_match(&name) {
                let number_string = name.trim_matches(|c: char| !c.is_ascii_digit());
                if let Ok(index) = number_string.parse::<usize>()
                    && index > 0
                {
                    if let Some(pigeonhole) = pigeonholes.get_mut(index - 1) {
                        *pigeonhole = false;
                    }
                }
            }
        }

        // compute the name
        let index = pigeonholes.iter().position(|b| *b).unwrap() + 1;
        let prefix = match component {
            VisualComponent::Textbox(_) => "Textbox",
            VisualComponent::Image(_) => "Image",
            VisualComponent::Pattern(_) => "Pattern",
        };
        format!("{prefix}{index}")
    }

    /// adds a component with the given name to the top
    /// returns true on success, fails if names are not unique
    pub fn push_component(&mut self, name: String, component: VisualComponent<C, R, V>) -> bool {
        if self.names.contains(&name) {
            return false;
        }

        self.names.push(name);
        self.components.push(component);

        true
    }

    /// gets the index of the component with the given name
    pub fn get_index(&self, name: &String) -> Option<usize> {
        self.names().position(|n| n == name)
    }

    /// removes the component with the given index
    /// returns the removed name and component on success
    pub fn remove(&mut self, index: usize) -> Option<(String, VisualComponent<C, R, V>)> {
        if self.names.len() <= index {
            return None;
        }

        Some((self.names.remove(index), self.components.remove(index)))
    }

    /// moves the component with the given index ot the top of the list
    /// returns true on success, fails if index is out of bounds
    pub fn to_top(&mut self, index: usize) -> bool {
        if self.names.len() <= index {
            return false;
        }

        let name = self.names.remove(index);
        let component = self.components.remove(index);
        self.names.push(name);
        self.components.push(component);
        true
    }

    /// moves the component with the given index ot the bottom
    /// returns true on success, fails if index is out of bounds
    pub fn to_bottom(&mut self, index: usize) -> bool {
        if self.names.len() <= index {
            return false;
        }

        let name = self.names.remove(index);
        let component = self.components.remove(index);
        self.names.insert(0, name);
        self.components.insert(0, component);
        true
    }

    /// gets the number of stored components
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// gets a reference to a component's name
    pub fn get_name(&self, index: usize) -> Option<&String> {
        self.names.get(index)
    }

    /// gets a mutable reference to a component's name
    pub fn get_name_mut(&mut self, index: usize) -> Option<&mut String> {
        self.names.get_mut(index)
    }

    /// gets a reference to a component
    pub fn get_component(&self, index: usize) -> Option<&VisualComponent<C, R, V>> {
        self.components.get(index)
    }

    /// gets a mutable reference to a component
    pub fn get_component_mut(&mut self, index: usize) -> Option<&mut VisualComponent<C, R, V>> {
        self.components.get_mut(index)
    }

    // an iterator over components
    pub fn components(&self) -> impl Iterator<Item = &VisualComponent<C, R, V>> {
        self.components.iter()
    }

    // an iterator over component names
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.names.iter()
    }

    // an iterator over mutable component references
    pub fn components_mut(&mut self) -> impl Iterator<Item = &mut VisualComponent<C, R, V>> {
        self.components.iter_mut()
    }

    // an iterator over mutable name references
    pub fn names_mut(&mut self) -> impl Iterator<Item = &mut String> {
        self.names.iter_mut()
    }

    // an iterator over components and names
    pub fn pairs(&self) -> impl Iterator<Item = (&String, &VisualComponent<C, R, V>)> {
        self.names.iter().zip(self.components.iter())
    }

    // a mutable iterator over components and names
    pub fn pairs_mut(
        &mut self,
    ) -> impl Iterator<Item = (&mut String, &mut VisualComponent<C, R, V>)> {
        self.names.iter_mut().zip(self.components.iter_mut())
    }

    // attempts to create a component from a lua table
    // returns a string error message on failure
    pub fn try_from_lua_table(table: Table) -> Result<Self, String> {
        let mut names = Vec::new();
        let mut components = Vec::new();
        let table_iter = table.sequence_values::<Table>();

        // read the component
        for (index, result) in table_iter.enumerate() {
            if result.is_err() {
                return Err(format!(
                    "Invalid component at index {index}: non-table value."
                ));
            }
            let unchecked_component = result.unwrap();

            // attempt to load the component's name
            match unchecked_component.get::<String>("name") {
                Ok(name) => {
                    if let Some(i) = names.iter().position(|s| *s == name) {
                        return Err(format!(
                            "Components at indices {i} and {index} have duplicate names."
                        ));
                    }
                    names.push(name);
                }

                Err(_) => {
                    return Err(format!(
                        "Invalid component at index {index}: Component has invalid name."
                    ));
                }
            }

            let component = match VisualComponent::<C, R, V>::try_from_lua_table(unchecked_component) {
                Ok(c) => c,
                Err(msg) => {
                    return Err(format!("Invalid component at index {index}: {msg}"));
                }
            };

            components.push(component);
        }

        Ok(Self { names, components })
    }
}

/// texture info for a piece face
pub struct TextureInfo<T> {
    /// the texture used to draw this piece
    pub texture: T,

    /// an enlarged version of this texture for zoomed views
    pub texture_magnified: T,
}
