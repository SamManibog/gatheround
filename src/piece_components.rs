use mlua::Table;

use crate::{
    color::{IColor, color_from_lua_table},
    geometry::{IRect, IVec2, rect_from_lua_table, vec2_from_lua_table},
    lua::FromLuaTable,
};

/// a transform for an image, only covers offset and scale
/// (scale is applied before offset)
#[derive(Debug, Clone)]
pub struct ImageTransform<V: IVec2> {
    pub offset: V,
    pub scale: f32,
}

impl<V: IVec2> FromLuaTable for ImageTransform<V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let offset = if let Ok(vec) = vec2_from_lua_table(table) {
            vec
        } else if let Ok(offset_table) = table.raw_get::<Table>("pos")
            && let Ok(vec) = vec2_from_lua_table(&offset_table)
        {
            vec
        } else {
            return Err("Missing pos field.".to_string());
        };

        let scale = match table.raw_get::<f64>("scale") {
            Ok(value) => value,
            Err(_) => {
                return Err("Invalid scale field.".to_string());
            }
        } as f32;

        Ok(Self { offset, scale })
    }
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

impl<V: IVec2> FromLuaTable for ImageInfo<V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let transform = if let Ok(t) = ImageTransform::<V>::try_from_lua_table(table) {
            t
        } else if let Ok(transform_table) = table.raw_get::<Table>("transform")
            && let Ok(t) = ImageTransform::<V>::try_from_lua_table(&transform_table)
        {
            t
        } else {
            return Err("Missing transform field.".to_string());
        };

        if let Ok(path) = table.raw_get::<String>("path") {
            Ok(Self { transform, path })
        } else {
            Err("Missing path field.".to_string())
        }
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

    pub fn try_from_string(string: &str) -> Result<Self, String> {
        match string {
            "topleft" => Ok(Self::TopLeft),
            "topcenter" => Ok(Self::TopCenter),
            "topright" => Ok(Self::TopRight),
            "centerleft" => Ok(Self::CenterLeft),
            "centercenter" => Ok(Self::CenterCenter),
            "centerright" => Ok(Self::CenterRight),
            "bottomleft" => Ok(Self::BottomLeft),
            "bottomcenter" => Ok(Self::BottomCenter),
            "bottomright" => Ok(Self::BottomRight),
            _ => Err(
                "Invalid text alignment. Must be one of: 'topleft', 'topcenter', 'topright', 'centerleft', 'centercenter', 'centerright', 'bottomleft', 'bottomcenter', or 'bottomright'.".to_string(),
            ),
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

impl<C: IColor, R: IRect> FromLuaTable for TextboxInfo<C, R> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let rect = if let Ok(r) = rect_from_lua_table(table) {
            r
        } else if let Ok(rect_table) = table.raw_get::<Table>("rect")
            && let Ok(r) = rect_from_lua_table(&rect_table)
        {
            r
        } else {
            return Err("Invalid rect field.".to_string());
        };

        let text = if let Ok(t) = table.raw_get::<String>("text") {
            if t.is_empty() {
                return Err("Empty text field.".to_string());
            }
            t
        } else {
            return Err("Invalid text field.".to_string());
        };

        let align = match TextAlign::try_from_string(
            &table
                .raw_get::<String>("align")
                .unwrap_or("centercenter".to_string()),
        ) {
            Ok(a) => a,
            Err(msg) => return Err(format!("Invalid align field: {msg}")),
        };

        let font_size = match table.raw_get::<mlua::Value>("font_size") {
            Ok(mlua::Value::Nil) => None,
            Ok(mlua::Value::Number(x)) => Some(x.clamp(1.0, u32::MAX as f64) as u32),
            Ok(mlua::Value::Integer(x)) => Some(x.clamp(1, u32::MAX as i64) as u32),
            Err(_) | Ok(_) => return Err(format!("Invalid size field, must be nil or numeric.")),
        };

        let line_spacing = match table.raw_get::<mlua::Value>("line_spacing") {
            Ok(mlua::Value::Nil) => None,
            Ok(mlua::Value::Number(x)) => Some(x.max(1.0) as f32),
            Ok(mlua::Value::Integer(x)) => Some(x.max(1) as f32),
            Err(_) | Ok(_) => return Err(format!("Invalid size field, must be nil or numeric.")),
        };

        let font_family = if let Ok(t) = table.raw_get::<String>("font") {
            t
        } else {
            return Err("Invalid font field.".to_string());
        };

        let color = if let Ok(color_table) = table.raw_get::<Table>("color") {
            match color_from_lua_table(&color_table) {
                Ok(c) => c,
                Err(msg) => return Err(format!("Invalid color field: {msg}")),
            }
        } else {
            return Err("Invalid color field.".to_string());
        };

        Ok(Self {
            rect,
            text,
            align,
            font_size,
            line_spacing,
            font_family,
            color,
        })
    }
}

/// a type of pattern
#[derive(Debug, Clone)]
pub enum PatternType {
    Solid,
    Checkers,
    Stripes,
    Dots,
}

impl PatternType {
    pub fn try_from_string(string: &str) -> Result<Self, String> {
        match string {
            "solid" => Ok(Self::Solid),
            "checkers" => Ok(Self::Checkers),
            "stripes" => Ok(Self::Stripes),
            "dots" => Ok(Self::Dots),
            _ => Err(
                "Invalid pattern type. Must be one of 'solid', 'checkers', 'stripes', or 'dots'"
                    .to_string(),
            ),
        }
    }
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

impl<C: IColor, R: IRect, V: IVec2> FromLuaTable for PatternInfo<C, R, V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let rect = if let Ok(r) = rect_from_lua_table(table) {
            r
        } else if let Ok(rect_table) = table.raw_get::<Table>("rect")
            && let Ok(r) = rect_from_lua_table(&rect_table)
        {
            r
        } else {
            return Err("Invalid rect field.".to_string());
        };

        let pattern = if let Ok(string) = table.raw_get::<String>("pattern") {
            match PatternType::try_from_string(&string) {
                Ok(p) => p,
                Err(msg) => return Err(format!("Invalid pattern field: {msg}")),
            }
        } else {
            return Err("Invalid pattern field.".to_string());
        };

        let weight = if let Ok(val) = table.raw_get::<f32>("weight") {
            val
        } else {
            return Err("Invalid weight field.".to_string());
        };

        let spacing = if let Ok(val) = table.raw_get::<f32>("spacing") {
            val
        } else {
            return Err("Invalid spacing field.".to_string());
        };

        let angle = if let Ok(val) = table.raw_get::<f32>("angle") {
            val
        } else {
            return Err("Invalid angle field.".to_string());
        };

        let offset = if let Ok(offset_table) = table.raw_get::<Table>("offset") {
            match vec2_from_lua_table(&offset_table) {
                Ok(val) => val,
                Err(msg) => return Err(format!("Invalid offset field: {msg}")),
            }
        } else {
            return Err("Invalid offset field.".to_string());
        };

        let pattern_color = if let Ok(color_table) = table.raw_get::<Table>("pattern_color") {
            match color_from_lua_table(&color_table) {
                Ok(c) => c,
                Err(msg) => return Err(format!("Invalid pattern_color field: {msg}")),
            }
        } else {
            return Err("Invalid pattern_color field.".to_string());
        };

        let background_color = if let Ok(color_table) = table.raw_get::<Table>("background_color") {
            match color_from_lua_table(&color_table) {
                Ok(c) => c,
                Err(msg) => return Err(format!("Invalid background_color field: {msg}")),
            }
        } else {
            return Err("Invalid background_color field.".to_string());
        };

        Ok(Self {
            rect,
            pattern,
            weight,
            spacing,
            angle,
            offset,
            pattern_color,
            background_color,
        })
    }
}

/// a visual element on a piece face
#[derive(Debug, Clone)]
pub enum VisualComponent<C: IColor, R: IRect, V: IVec2> {
    Textbox(Box<TextboxInfo<C, R>>),
    Image(Box<ImageInfo<V>>),
    Pattern(Box<PatternInfo<C, R, V>>),
}

impl<C: IColor, R: IRect, V: IVec2> FromLuaTable for VisualComponent<C, R, V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        match table.raw_get::<String>("type") {
            Ok(t) => {
                if t == "textbox" {
                    match TextboxInfo::try_from_lua_table(table) {
                        Ok(val) => Ok(Self::Textbox(Box::new(val))),
                        Err(msg) => Err(format!("Invalid textbox: {msg}")),
                    }
                } else if t == "image" {
                    match ImageInfo::try_from_lua_table(table) {
                        Ok(val) => Ok(Self::Image(Box::new(val))),
                        Err(msg) => Err(format!("Invalid image: {msg}")),
                    }
                } else if t == "pattern" {
                    match PatternInfo::try_from_lua_table(table) {
                        Ok(val) => Ok(Self::Pattern(Box::new(val))),
                        Err(msg) => Err(format!("Invalid pattern: {msg}")),
                    }
                } else {
                    Err(format!(
                        "Component has invalid type '{t}', must be 'textbox', 'image', or 'pattern'."
                    ))
                }
            }

            Err(_) => Err(format!("Component has invalid type.")),
        }
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

    /// adds a component to the top of the visual
    pub fn push_component(&mut self, component: VisualComponent<C, R, V>) {
        self.components.push(component);
    }

    /// removes the component with the given index
    /// returns the removed name and component on success
    pub fn remove(&mut self, index: usize) -> Option<VisualComponent<C, R, V>> {
        Some(self.components.remove(index))
    }

    /// moves the component with the given index ot the top of the list
    /// returns true on success, fails if index is out of bounds
    pub fn to_top(&mut self, index: usize) {
        let component = self.components.remove(index);
        self.components.push(component);
    }

    /// moves the component with the given index ot the bottom
    /// returns true on success, fails if index is out of bounds
    pub fn to_bottom(&mut self, index: usize) {
        let component = self.components.remove(index);
        self.components.insert(0, component);
    }

    /// gets the number of stored components
    pub fn len(&self) -> usize {
        self.components.len()
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

    // an iterator over mutable component references
    pub fn components_mut(&mut self) -> impl Iterator<Item = &mut VisualComponent<C, R, V>> {
        self.components.iter_mut()
    }
}

impl<C: IColor, R: IRect, V: IVec2> FromLuaTable for VisualInfo<C, R, V> {
    // attempts to create a component from a lua table
    // returns a string error message on failure
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let mut components = Vec::new();
        let table_iter = table.sequence_values::<Table>();

        // read the component
        for (index, result) in table_iter.enumerate() {
            if result.is_err() {
                return Err(format!(
                    "Invalid component at index {}: non-table value.",
                    index + 1
                ));
            }
            let unchecked_component = result.unwrap();

            let component =
                match VisualComponent::<C, R, V>::try_from_lua_table(&unchecked_component) {
                    Ok(c) => c,
                    Err(msg) => {
                        return Err(format!("Invalid component at index {}: {msg}", index + 1));
                    }
                };

            components.push(component);
        }

        Ok(Self { components })
    }
}

/// texture info for a piece face
#[derive(Debug, Clone)]
pub struct TextureInfo<T> {
    /// the texture used to draw this piece
    pub texture: T,

    /// an enlarged version of this texture for zoomed views
    pub texture_magnified: T,
}
