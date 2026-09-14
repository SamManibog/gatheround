use std::fmt::Debug;

use mlua::{Table, Value};

use crate::{
    color::{IColor, color_from_lua_value},
    ecs::EntityTrait,
    geometry::{
        CollisionShape, IConvexShape, IRect, IVec2, RigidTransform, finite_vec2_from_lua_table,
        finite_vec2_from_lua_value, valid_finite_rect_from_lua_table,
    },
    lua::{FromLuaTable, FromLuaValue, finite_f32_from_lua_value, vector_from_lua_value},
    resources::CollisionShapeEntity,
};

/// a transform for an image, only covers offset and scale
/// (scale is applied before position)
#[derive(Debug, Clone)]
pub struct ImageTransform<V: IVec2> {
    pub position: V,
    pub scale: f32,
}

impl<V: IVec2> FromLuaTable for ImageTransform<V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let position = if let Ok(vec) = finite_vec2_from_lua_table(table) {
            vec
        } else if let Ok(position_table) = table.raw_get::<Table>("pos")
            && let Ok(vec) = finite_vec2_from_lua_table(&position_table)
        {
            vec
        } else {
            return Err("Missing pos field.".to_string());
        };

        let scale = match table.raw_get::<Value>("scale") {
            Ok(value) => {
                if value.is_nil() {
                    1.0
                } else {
                    match finite_f32_from_lua_value(&value) {
                        Ok(v) => v,
                        Err(msg) => return Err(format!("Invalid scale field: {msg}")),
                    }
                }
            }
            Err(msg) => return Err(format!("Invalid scale field: {msg}")),
        };

        Ok(Self { position, scale })
    }
}

impl<V: IVec2> ImageTransform<V> {
    pub fn identity() -> Self {
        Self {
            position: V::from_xy(0.0, 0.0),
            scale: 1.0,
        }
    }

    pub fn apply_to_point<T: IVec2>(&self, vector: T) -> T {
        T::from_other(vector.scaled(self.scale).add(self.position))
    }

    pub fn apply_to_rect<R: IRect>(&self, rect: R) -> R {
        let min: (f32, f32) = self.apply_to_point(rect.min::<(f32, f32)>());
        let max: (f32, f32) = self.apply_to_point(rect.max::<(f32, f32)>());

        R::from_min_max(min, max)
    }

    pub fn apply_to_other<T: IVec2>(
        &self,
        other: &ImageTransform<T>,
    ) -> ImageTransform<T> {
        ImageTransform::<T> {
            position: self.apply_to_point(other.position),
            scale: self.scale * other.scale,
        }
    }
}

/// information required to put an image on a piece
#[derive(Debug, Clone)]
pub struct ImageInfo<C: IColor, V: IVec2> {
    pub transform: ImageTransform<V>,
    pub path: String,
    pub tint: C,
}

impl<C: IColor, V: IVec2> FromLuaTable for ImageInfo<C, V> {
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

        let path = if let Ok(p) = table.raw_get::<String>("path") {
            p
        } else {
            return Err("Missing path field.".to_string());
        };

        let tint = if let Ok(tint_val) = table.raw_get::<Value>("tint") {
            match tint_val {
                Value::Nil => C::from_rgba(255, 255, 255, 255),
                _ => color_from_lua_value(&tint_val)?,
            }
        } else {
            return Err("Invalid tint field.".to_string());
        };

        Ok(Self {
            tint,
            transform,
            path,
        })
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
    pub size: Option<u32>,

    /// the font's line spacing as a percent of font height
    /// None for automatic
    pub spacing: Option<f32>,

    /// the font's identifier, maybe a path or name
    pub font: String,

    /// the font's color
    pub color: C,
}

impl<C: IColor, R: IRect> FromLuaTable for TextboxInfo<C, R> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let rect = if let Ok(r) = valid_finite_rect_from_lua_table(table) {
            r
        } else if let Ok(rect_table) = table.raw_get::<Table>("rect")
            && let Ok(r) = valid_finite_rect_from_lua_table(&rect_table)
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

        let align = match table.raw_get::<mlua::Value>("align") {
            Ok(a) => {
                if a == Value::Nil {
                    TextAlign::CenterCenter
                } else {
                    match String::try_from_lua_value(&a) {
                        Ok(s) => match TextAlign::try_from_string(&s) {
                            Ok(a) => a,
                            Err(msg) => return Err(format!("Invalid align field: {msg}")),
                        },
                        Err(msg) => return Err(format!("Invalid align field: {msg}")),
                    }
                }
            }
            Err(_) => return Err(format!("Invalid align field.")),
        };

        let size = match table.raw_get::<mlua::Value>("size") {
            Ok(mlua::Value::Nil) => None,
            Ok(mlua::Value::Number(x)) => Some(x.clamp(1.0, u32::MAX as f64) as u32),
            Ok(mlua::Value::Integer(x)) => Some(x.clamp(1, u32::MAX as i64) as u32),
            Err(_) | Ok(_) => return Err(format!("Invalid size field, must be nil or numeric.")),
        };

        let spacing = match table.raw_get::<mlua::Value>("spacing") {
            Ok(mlua::Value::Nil) => None,
            Ok(val) => match finite_f32_from_lua_value(&val) {
                Ok(v) => Some(v),
                Err(msg) => return Err(format!("Invalid spacing field: {msg}")),
            },
            Err(msg) => return Err(format!("Invalid spacing field: {msg}")),
        };

        let font = if let Ok(t) = table.raw_get::<String>("font") {
            t
        } else {
            return Err("Invalid font field.".to_string());
        };

        let color = if let Ok(color_val) = table.raw_get::<Value>("color") {
            match color_val {
                Value::Nil => C::from_rgba(0, 0, 0, 255),
                _ => color_from_lua_value(&color_val)?,
            }
        } else {
            return Err("Invalid color field.".to_string());
        };

        Ok(Self {
            rect,
            text,
            align,
            size,
            spacing,
            font,
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

    pub fn default_weight(&self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Checkers => 10.0,
            Self::Stripes => 10.0,
            Self::Dots => 8.0,
        }
    }

    pub fn default_spacing(&self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Checkers => 0.0,
            Self::Stripes => 10.0,
            Self::Dots => 8.0,
        }
    }

    pub fn default_angle(&self) -> f32 {
        match self {
            Self::Solid => 0.0,
            Self::Checkers => 0.0,
            Self::Stripes => 45.0,
            Self::Dots => 45.0,
        }
    }

    pub fn default_offset<V: IVec2>(&self) -> V {
        match self {
            Self::Solid => V::from_xy(0.0, 0.0),
            Self::Checkers => V::from_xy(0.0, 0.0),
            Self::Stripes => V::from_xy(0.0, 0.0),
            Self::Dots => V::from_xy(0.0, 0.0),
        }
    }

    pub fn default_pattern_color<C: IColor>(&self) -> C {
        match self {
            Self::Solid => C::from_rgba(0, 0, 0, 0),
            Self::Checkers => C::from_rgba(0, 0, 0, 0),
            Self::Stripes => C::from_rgba(0, 0, 0, 0),
            Self::Dots => C::from_rgba(0, 0, 0, 0),
        }
    }

    pub fn default_background_color<C: IColor>(&self) -> C {
        match self {
            Self::Solid => C::from_rgba(255, 255, 255, 255),
            Self::Checkers => C::from_rgba(255, 255, 255, 255),
            Self::Stripes => C::from_rgba(255, 255, 255, 255),
            Self::Dots => C::from_rgba(255, 255, 255, 255),
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
    /// in degrees
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
    /// returns the offset recomputed based on the pattern
    pub fn true_offset(&self) -> V {
        match self.pattern {
            PatternType::Solid => V::from_xy(0.0, 0.0),
            PatternType::Checkers => {
                let repetition_length = (self.weight + self.spacing) * 2.0;
                V::from_xy(
                    self.offset.x().rem_euclid(repetition_length),
                    self.offset.y().rem_euclid(repetition_length),
                )
            }
            PatternType::Stripes => {
                let repetition_length = self.weight + self.spacing;
                V::from_xy(self.offset.x().rem_euclid(repetition_length), 0.0)
            }
            PatternType::Dots => {
                let repetition_length = self.weight + self.spacing;
                V::from_xy(
                    self.offset.x().rem_euclid(repetition_length),
                    self.offset.y().rem_euclid(repetition_length),
                )
            }
        }
    }
}

impl<C: IColor, R: IRect, V: IVec2> FromLuaTable for PatternInfo<C, R, V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let rect = if let Ok(r) = valid_finite_rect_from_lua_table(table) {
            r
        } else if let Ok(rect_table) = table.raw_get::<Table>("rect")
            && let Ok(r) = valid_finite_rect_from_lua_table(&rect_table)
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

        let weight = match table.raw_get::<Value>("weight") {
            Ok(value) => {
                if value.is_nil() {
                    pattern.default_weight()
                } else {
                    match finite_f32_from_lua_value(&value) {
                        Ok(v) => v,
                        Err(msg) => return Err(format!("Invalid weight field: {msg}")),
                    }
                }
            }
            Err(msg) => return Err(format!("Invalid weight field: {msg}")),
        };

        let spacing = match table.raw_get::<Value>("spacing") {
            Ok(value) => {
                if value.is_nil() {
                    pattern.default_spacing()
                } else {
                    match finite_f32_from_lua_value(&value) {
                        Ok(v) => v,
                        Err(msg) => return Err(format!("Invalid spacing field: {msg}")),
                    }
                }
            }
            Err(msg) => return Err(format!("Invalid spacing field: {msg}")),
        };

        let angle = match table.raw_get::<Value>("angle") {
            Ok(value) => {
                if value.is_nil() {
                    pattern.default_angle()
                } else {
                    match finite_f32_from_lua_value(&value) {
                        Ok(v) => v,
                        Err(msg) => return Err(format!("Invalid angle field: {msg}")),
                    }
                }
            }
            Err(msg) => return Err(format!("Invalid angle field: {msg}")),
        };

        let offset = if let Ok(offset_table_opt) = table.raw_get::<Option<Table>>("offset") {
            if let Some(offset_table) = offset_table_opt {
                match finite_vec2_from_lua_table(&offset_table) {
                    Ok(val) => val,
                    Err(msg) => return Err(format!("Invalid offset field: {msg}")),
                }
            } else {
                pattern.default_offset()
            }
        } else {
            return Err("Invalid offset field.".to_string());
        };

        let pattern_color = if let Ok(color_val) = table.raw_get::<Value>("pattern_color") {
            match color_val {
                Value::Nil => pattern.default_pattern_color(),
                _ => color_from_lua_value(&color_val)?,
            }
        } else {
            return Err("Invalid pattern_color field.".to_string());
        };

        let background_color = if let Ok(color_val) = table.raw_get::<Value>("background_color") {
            match color_val {
                Value::Nil => pattern.default_background_color(),
                _ => color_from_lua_value(&color_val)?,
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
    Image(Box<ImageInfo<C, V>>),
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

/// information for a visual element on a piece face
#[derive(Debug, Clone)]
pub struct VisualInfo<C: IColor, R: IRect, V: IVec2> {
    /// the visual components for the piece, in order from bottom to top
    pub components: Vec<VisualComponent<C, R, V>>,
}

impl<C: IColor, R: IRect, V: IVec2> From<VisualComponent<C, R, V>> for VisualInfo<C, R, V> {
    fn from(value: VisualComponent<C, R, V>) -> Self {
        Self {
            components: vec![value],
        }
    }
}

impl<C: IColor, R: IRect, V: IVec2> From<TextboxInfo<C, R>> for VisualComponent<C, R, V> {
    fn from(value: TextboxInfo<C, R>) -> Self {
        VisualComponent::Textbox(Box::new(value))
    }
}

impl<C: IColor, R: IRect, V: IVec2> From<ImageInfo<C, V>> for VisualComponent<C, R, V> {
    fn from(value: ImageInfo<C, V>) -> Self {
        VisualComponent::Image(Box::new(value))
    }
}

impl<C: IColor, R: IRect, V: IVec2> From<PatternInfo<C, R, V>> for VisualComponent<C, R, V> {
    fn from(value: PatternInfo<C, R, V>) -> Self {
        VisualComponent::Pattern(Box::new(value))
    }
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
            VisualComponent::Textbox(t) => Some(&t.font),
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

        if components.is_empty() {
            match VisualComponent::try_from_lua_table(table) {
                Ok(component) => Ok(Self {
                    components: vec![component],
                }),
                Err(msg) => Err(msg),
            }
        } else {
            Ok(Self { components })
        }
    }
}

/// an identifier for an entity, which may either be by name or entity id
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityIdentifier<E: EntityTrait> {
    Entity(E),
    Name(String),
}

impl<E: EntityTrait> From<E> for EntityIdentifier<E> {
    fn from(value: E) -> Self {
        Self::Entity(value)
    }
}

impl<E: EntityTrait> From<String> for EntityIdentifier<E> {
    fn from(value: String) -> Self {
        Self::Name(value)
    }
}

impl<E: EntityTrait> FromLuaValue for EntityIdentifier<E> {
    fn try_from_lua_value(value: &mlua::Value) -> Result<Self, String> {
        if let mlua::Value::String(s) = value {
            Ok(Self::Name(format!("{}", s.display())))
        } else {
            Ok(Self::Entity(E::try_from_lua_value(value)?))
        }
    }
}

/// collision shape data intermediary between lua tables and actual collision shape
#[derive(Debug, Clone)]
pub enum CollisionShapeIR<S: IConvexShape + FromLuaTable + Debug> {
    Identifier(EntityIdentifier<CollisionShapeEntity>),
    Instance(CollisionShape<S>),
}

impl<S: IConvexShape + FromLuaTable + Debug, I: Into<EntityIdentifier<CollisionShapeEntity>>>
    From<I> for CollisionShapeIR<S>
{
    fn from(value: I) -> Self {
        let ident: EntityIdentifier<CollisionShapeEntity> = value.into();
        Self::Identifier(ident)
    }
}

impl<S: IConvexShape + FromLuaTable + Debug> From<CollisionShape<S>> for CollisionShapeIR<S> {
    fn from(value: CollisionShape<S>) -> Self {
        Self::Instance(value)
    }
}

impl<S: IConvexShape + FromLuaTable + Debug> FromLuaValue for CollisionShapeIR<S> {
    fn try_from_lua_value(value: &mlua::Value) -> Result<Self, String> {
        if let mlua::Value::Table(table) = value {
            Ok(Self::Instance(CollisionShape::try_from_lua_table(table)?))
        } else {
            Ok(Self::Identifier(EntityIdentifier::try_from_lua_value(
                value,
            )?))
        }
    }
}

#[derive(Debug, Clone)]
pub struct PileIR<V: IVec2> {
    /// the name of the pile
    pub name: Option<String>,

    /// the transform of the pile
    pub transform: RigidTransform<V>,

    /// categorizations
    pub tags: Vec<String>,

    /// the relative of the snapping circle
    pub snap_offset: V,

    /// the radius of the snapping cirle
    /// circle is placed relative to offset of the top piece
    pub snap_radius: f32,

    /// the draw layer of the pile
    pub draw_layer: i32,
}

impl<V: IVec2> FromLuaValue for PileIR<V> {
    fn try_from_lua_value(value: &Value) -> Result<Self, String> {
        match value {
            Value::Table(table) => Self::try_from_lua_table(&table),
            _ => Err(format!("non-table value.")),
        }
    }
}

impl<V: IVec2> FromLuaTable for PileIR<V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let name = if let Ok(val) = table.raw_get::<Value>("name") {
            if let Value::Nil = val {
                None
            } else {
                match String::try_from_lua_value(&val) {
                    Ok(t) => Some(t),
                    Err(msg) => return Err(format!("Invalid name field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid name field."));
        };

        let transform = if let Ok(val) = table.raw_get::<Value>("transform") {
            if let Value::Nil = val {
                Default::default()
            } else {
                match RigidTransform::try_from_lua_value(&val) {
                    Ok(t) => t,
                    Err(msg) => return Err(format!("Invalid transform field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid transform field."));
        };

        let snap_offset = if let Ok(val) = table.raw_get::<Value>("snap_offset") {
            if let Value::Nil = val {
                V::from_xy(0.0, 0.0)
            } else {
                match finite_vec2_from_lua_value(&val) {
                    Ok(t) => t,
                    Err(msg) => return Err(format!("Invalid snap_offset field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid position field."));
        };

        let snap_radius = if let Ok(value) = table.raw_get::<Value>("snap_radius") {
            match &value {
                Value::Nil => 0.0,
                Value::Number(float) => *float as f32,
                Value::Integer(int) => *int as f32,
                _ => return Err(format!("Invalid snap_radius field: non-numeric value.")),
            }
        } else {
            return Err(format!("Invalid snap_radius field."));
        };

        let draw_layer = if let Ok(value) = table.raw_get::<Value>("draw_layer") {
            match &value {
                Value::Nil => 0,
                Value::Number(float) => {
                    if float.round() != *float {
                        return Err(format!("Invalid draw_layer field: non-integer value."));
                    }
                    if float.clamp(i32::MIN as f64, i32::MAX as f64) != *float {
                        return Err(format!(
                            "Invalid draw_layer field: must be in range [{}, {}].",
                            i32::MIN,
                            i32::MAX
                        ));
                    }
                    *float as i32
                }
                Value::Integer(int) => {
                    if (*int).clamp(i32::MIN as i64, i32::MAX as i64) != *int {
                        return Err(format!(
                            "Invalid draw_layer field: must be in range [{}, {}].",
                            i32::MIN,
                            i32::MAX
                        ));
                    }
                    *int as i32
                }
                _ => return Err(format!("Invalid draw_layer field: non-integer value.")),
            }
        } else {
            return Err(format!("Invalid draw_layer field."));
        };

        let tags = if let Ok(value) = table.raw_get::<Value>("tags") {
            vector_from_lua_value(&value, "tags field", "tag")?
        } else {
            return Err(format!("Invalid tags field."));
        };

        Ok(Self {
            name,
            transform,
            snap_offset,
            snap_radius,
            tags,
            draw_layer,
        })
    }
}

/// collider data that serves as an intermediary between lua tables and a pieces collider
#[derive(Debug, Clone)]
pub struct PieceColliderIR<S: IConvexShape + FromLuaTable + Debug> {
    /// the collision shape entity
    pub shape: CollisionShapeIR<S>,

    /// the collision layer this collider belongs on
    pub layer: String,
}

impl<S: IConvexShape + FromLuaTable + Debug> FromLuaValue for PieceColliderIR<S> {
    fn try_from_lua_value(value: &mlua::prelude::LuaValue) -> Result<Self, String> {
        match value {
            Value::Table(table) => Self::try_from_lua_table(&table),
            _ => Err(format!("non-table value.")),
        }
    }
}

impl<S: IConvexShape + FromLuaTable + Debug> FromLuaTable for PieceColliderIR<S> {
    fn try_from_lua_table(table: &mlua::prelude::LuaTable) -> Result<Self, String> {
        let shape = if let Ok(shape_table) = table.raw_get::<Value>("shape") {
            match CollisionShapeIR::try_from_lua_value(&shape_table) {
                Ok(shape) => shape,
                Err(msg) => return Err(format!("Invalid shape field: {msg}")),
            }
        } else {
            return Err(format!("Invalid shape field."));
        };

        let layer = if let Ok(value) = table.raw_get::<Value>("layer") {
            if value == Value::Nil {
                String::from("default")
            } else {
                match String::try_from_lua_value(&value) {
                    Ok(v) => v,
                    Err(msg) => return Err(format!("Invalid layer field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid layer field."));
        };

        Ok(Self { shape, layer })
    }
}

/// piece data that serves as an intermediary between lua tables an a piece on the board
pub struct PieceIR<S: IConvexShape + FromLuaTable + Debug, V: IVec2> {
    /// optional name
    pub name: Option<String>,

    /// texture
    pub texture: String,

    /// the transform for the piece
    pub transform: RigidTransform<V>,

    /// colliders
    pub colliders: Vec<PieceColliderIR<S>>,

    /// draw layer
    pub draw_layer: i32,

    /// categorizations
    pub tags: Vec<String>,
}

impl<S: IConvexShape + FromLuaTable + Debug, V: IVec2> FromLuaValue for PieceIR<S, V> {
    fn try_from_lua_value(value: &mlua::prelude::LuaValue) -> Result<Self, String> {
        match value {
            Value::Table(table) => Self::try_from_lua_table(&table),
            _ => Err(format!("non-table value.")),
        }
    }
}

impl<S: IConvexShape + FromLuaTable + Debug, V: IVec2> FromLuaTable for PieceIR<S, V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let name = if let Ok(val) = table.raw_get::<Value>("name") {
            if let Value::Nil = val {
                None
            } else {
                match String::try_from_lua_value(&val) {
                    Ok(t) => Some(t),
                    Err(msg) => return Err(format!("Invalid name field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid name field."));
        };

        let texture = if let Ok(val) = table.raw_get::<Value>("texture") {
            match String::try_from_lua_value(&val) {
                Ok(t) => t,
                Err(msg) => return Err(format!("Invalid texture field: {msg}")),
            }
        } else {
            return Err(format!("Invalid name field."));
        };

        let transform = if let Ok(val) = table.raw_get::<Value>("transform") {
            if let Value::Nil = val {
                Default::default()
            } else {
                match RigidTransform::try_from_lua_value(&val) {
                    Ok(t) => t,
                    Err(msg) => return Err(format!("Invalid transform field: {msg}")),
                }
            }
        } else {
            return Err(format!("Invalid transform field."));
        };

        let colliders = if let Ok(value) = table.raw_get::<Value>("colliders") {
            vector_from_lua_value(&value, "collider field", "collider")?
        } else {
            return Err(format!("Invalid collider field."));
        };

        let draw_layer = if let Ok(value) = table.raw_get::<Value>("draw_layer") {
            match &value {
                Value::Nil => 0,
                Value::Number(float) => {
                    if float.round() != *float {
                        return Err(format!("Invalid draw_layer field: non-integer value."));
                    }
                    if float.clamp(i32::MIN as f64, i32::MAX as f64) != *float {
                        return Err(format!(
                            "Invalid draw_layer field: must be in range [{}, {}].",
                            i32::MIN,
                            i32::MAX
                        ));
                    }
                    *float as i32
                }
                Value::Integer(int) => {
                    if (*int).clamp(i32::MIN as i64, i32::MAX as i64) != *int {
                        return Err(format!(
                            "Invalid draw_layer field: must be in range [{}, {}].",
                            i32::MIN,
                            i32::MAX
                        ));
                    }
                    *int as i32
                }
                _ => return Err(format!("Invalid draw_layer field: non-integer value.")),
            }
        } else {
            return Err(format!("Invalid draw_layer field."));
        };

        let tags = if let Ok(value) = table.raw_get::<Value>("tags") {
            vector_from_lua_value(&value, "tags field", "tag")?
        } else {
            return Err(format!("Invalid tags field."));
        };

        Ok(Self {
            name,
            texture,
            transform,
            colliders,
            draw_layer,
            tags,
        })
    }
}
