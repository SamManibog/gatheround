use std::{cmp::Ordering, fmt::Debug};

use mlua::{Lua, Table, Value};

use crate::lua::{FromLuaTable, FromLuaValue, finite_f32_from_lua_value};

pub const GJK_MAX_ITERATIONS: u32 = 64;

pub trait AsF32 {
    fn as_f32(self) -> f32;
}
macro_rules! impl_as_f32 {
    ($($t:ty),*) => {
        $(
            impl AsF32 for $t {
                fn as_f32(self) -> f32 {
                    self as f32
                }
            }
        )*
    };
}
impl_as_f32!(i8, u8, i16, u16, i32, u32, i64, u64, isize, usize, f32, f64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BasicRect {
    x: f32,
    y: f32, 
    width: f32,
    height: f32,
}

impl IRect for BasicRect {
    fn x(&self) -> f32 {
        self.x
    }

    fn y(&self) -> f32 {
        self.y
    }

    fn width(&self) -> f32 {
        self.width
    }

    fn height(&self) -> f32 {
        self.height
    }

    fn from_position_dimension<V1: IVec2, V2: IVec2>(position: V1, dimensions: V2) -> Self {
        Self {
            x: position.x(),
            y: position.y(),
            width: dimensions.x(),
            height: dimensions.y(),
        }
    }
}

impl IConvexShape for BasicRect {
    fn furthest_point<V: IVec2>(&self, direction: V) -> V {
        IRect::furthest_point(self, direction)
    }
}

/// totally orded subset of floating-point numbers
/// only supporting comparison operations
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct NumericF32(f32);

impl Eq for NumericF32 {}

impl Ord for NumericF32 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap()
    }
}

impl NumericF32 {
    /// creates a new totally ordered f32, panicing if a NaN value is passed
    pub fn new(val: f32) -> Self {
        assert!(
            !val.is_nan(),
            "Attempted to create an ordered f32 out of a NaN value."
        );
        Self(val)
    }
}

/// attempt to instance from a lua value
pub fn finite_vec2_from_lua_value<V: IVec2>(value: &mlua::Value) -> Result<V, String> {
    match value {
        Value::Table(table) => finite_vec2_from_lua_table(&table),
        _ => Err(format!("non-table value.")),
    }
}

/// attempt to instance from a lua table
pub fn finite_vec2_from_lua_table<V: IVec2>(table: &Table) -> Result<V, String> {
    let x = match table.raw_get::<Value>("x") {
        Ok(value) => match finite_f32_from_lua_value(&value) {
            Ok(v) => v,
            Err(msg) => return Err(format!("Invalid x field: {msg}")),
        },
        Err(msg) => return Err(format!("Invalid x field: {msg}")),
    };

    let y = match table.raw_get::<Value>("y") {
        Ok(value) => match finite_f32_from_lua_value(&value) {
            Ok(v) => v,
            Err(msg) => return Err(format!("Invalid y field: {msg}")),
        },
        Err(msg) => return Err(format!("Invalid y field: {msg}")),
    };

    Ok(V::from_xy(x, y))
}

/// trait representing 2d vectors
pub trait IVec2: std::fmt::Debug + Sized + Clone + Copy + PartialEq + IConvexShape {
    /// instance this from an x and y coordinate
    fn from_xy(x: f32, y: f32) -> Self;

    /// instance from another vector
    fn from_other<V: IVec2>(other: V) -> Self {
        Self::from_xy(other.x(), other.y())
    }

    /// creates a lua table from this vector
    fn into_lua_table(&self, lua: &Lua) -> mlua::Result<mlua::Table> {
        let table = lua.create_table_with_capacity(0, 2)?;
        table.raw_set("x", self.x())?;
        table.raw_set("y", self.y())?;
        Ok(table)
    }

    /// the origin
    fn origin() -> Self {
        Self::from_xy(0.0, 0.0)
    }

    /// create an instance from this one, setting x value
    fn with_x(self, x: f32) -> Self {
        Self::from_xy(x, self.y())
    }

    /// create an instance from this one, setting y value
    fn with_y(self, y: f32) -> Self {
        Self::from_xy(self.x(), y)
    }

    /// create an isntance from this one, negating x value
    fn neg_x(self) -> Self {
        Self::from_xy(-self.x(), self.y())
    }

    /// create an isntance from this one, negating y value
    fn neg_y(self) -> Self {
        Self::from_xy(self.x(), -self.y())
    }

    /// create an isntance from this one where both x and y components are non-negative
    fn abs(&self) -> Self {
        Self::from_xy(self.x().abs(), self.y().abs())
    }

    /// rounds to the nearest non-fractional vector
    fn round(&self) -> Self {
        Self::from_xy(self.x().round(), self.y().round())
    }

    /// create an isntance from this one, modifying x value by the given delta
    fn change_x(self, delta: f32) -> Self {
        Self::from_xy(self.x() + delta, self.y())
    }

    /// create an isntance from this one, modifying y value by the given delta
    fn change_y(self, delta: f32) -> Self {
        Self::from_xy(self.x(), self.y() + delta)
    }

    /// create an isntance from this one, scaling x value by the given delta
    fn scale_x(self, factor: f32) -> Self {
        Self::from_xy(self.x() * factor, self.y())
    }

    /// create an isntance from this one, scalling y value by the given delta
    fn scale_y(self, factor: f32) -> Self {
        Self::from_xy(self.x(), self.y() * factor)
    }

    /// get the x position of the transform
    fn x(&self) -> f32;

    /// get the y position of the transform
    fn y(&self) -> f32;

    /// gets the dot product of this vector and another
    fn dot<V: IVec2>(&self, other: V) -> f32 {
        self.x() * other.x() + self.y() * other.y()
    }

    /// returns true if both x and y components are non-negaive
    fn is_non_negative(&self) -> bool {
        self.x() >= 0.0 && self.y() >= 0.0
    }

    /// gets the cross product of three points, with this point one denoting the origin
    fn cross<V1: IVec2, V2: IVec2>(&self, a: V1, b: V2) -> f32 {
        (a.x() - self.x()) * (b.y() - self.y()) - (a.y() - self.y()) * (b.x() - self.x())
    }

    /// gets a scaled version of this by a f32
    fn scaled(&self, factor: f32) -> Self {
        Self::from_xy(self.x() * factor, self.y() * factor)
    }

    /// gets the distance between this vector and the origin
    fn length(&self) -> f32 {
        f32::sqrt(self.x() * self.x() + self.y() * self.y())
    }

    /// gets the distance between two points
    fn distance_to<V: IVec2>(&self, other: V) -> f32 {
        self.sub(other).length()
    }

    /// gets the normalized version of this vector
    fn normalized(&self) -> Self {
        self.scaled(1.0f32 / self.length())
    }

    /// gets a normal of the line that connects this point to another
    fn normal<V: IVec2>(&self, other: V) -> Self {
        Self::from_xy(self.y() - other.y(), other.x() - self.x())
    }

    /// gets the normal vector that points most towards the origin for the line that connects this point to another
    fn origin_normal<V: IVec2>(&self, other: V) -> Self {
        let normal = self.normal(other);
        normal.scaled(-normal.dot(*self).signum())
    }

    /// gets the angle of a vector from the positive x-axis
    fn angle(&self) -> f32 {
        self.y().atan2(self.x())
    }

    /// rotates by the given angle
    fn rotated(&self, angle: f32) -> Self {
        let cos_theta = angle.cos();
        let sin_theta = angle.sin();

        Self::from_xy(
            self.x() * cos_theta - self.y() * sin_theta,
            self.x() * sin_theta + self.y() * cos_theta,
        )
    }

    /// adds the two vectors together
    fn add<V: IVec2>(&self, other: V) -> Self {
        Self::from_xy(self.x() + other.x(), self.y() + other.y())
    }

    /// subtracts the following vector from this one
    fn sub<V: IVec2>(&self, other: V) -> Self {
        Self::from_xy(self.x() - other.x(), self.y() - other.y())
    }

    /// negate this vector
    fn neg(&self) -> Self {
        Self::from_xy(-self.x(), -self.y())
    }
}

impl IVec2 for (f32, f32) {
    fn from_xy(x: f32, y: f32) -> Self {
        (x, y)
    }

    fn x(&self) -> f32 {
        self.0
    }

    fn y(&self) -> f32 {
        self.1
    }
}

impl IConvexShape for (f32, f32) {
    fn furthest_point<V: IVec2>(&self, _direction: V) -> V {
        V::from_other(*self)
    }
}

/// attempt to instance from a lua table
/// x, y, width, and height must be finite
/// width and height must be non-negative
pub fn valid_finite_rect_from_lua_table<R: IRect>(table: &Table) -> Result<R, String> {
    let position = finite_vec2_from_lua_table::<(f32, f32)>(table)?;

    let width = match table.raw_get::<Value>("width") {
        Ok(value) => match finite_f32_from_lua_value(&value) {
            Ok(v) => v,
            Err(msg) => return Err(format!("Invalid width field: {msg}")),
        },
        Err(msg) => return Err(format!("Invalid width field: {msg}")),
    };

    if width < 0.0 {
        return Err(format!("Invalid width field: got negative value."));
    }

    let height = match table.raw_get::<Value>("height") {
        Ok(value) => match finite_f32_from_lua_value(&value) {
            Ok(v) => v,
            Err(msg) => return Err(format!("Invalid height field: {msg}")),
        },
        Err(msg) => return Err(format!("Invalid height field: {msg}")),
    };

    if height < 0.0 {
        return Err(format!("Invalid height field: got negative value."));
    }

    Ok(R::from_position_dimension(
        position,
        (width as f32, height as f32),
    ))
}

pub trait IRect: Sized + Clone + Copy + IConvexShape + Debug {
    /// creates an instance from the top left corner and dimensions
    /// should panic if dimensions are negative
    fn from_position_dimension<V1: IVec2, V2: IVec2>(position: V1, dimensions: V2) -> Self;

    /// gets the furthest point on the rectangle in the direction of the given vector
    fn furthest_point<V: IVec2>(&self, direction: V) -> V {
        let mut max_point = self.top_left::<V>();
        let mut max_dot = max_point.dot(direction);

        for point in [
            self.top_right::<V>(),
            self.bottom_right(),
            self.bottom_left(),
        ] {
            let dot = point.dot(direction);
            if dot > max_dot {
                max_dot = dot;
                max_point = point;
            }
        }
        max_point
    }

    /// creates an instance from the center and dimensions
    fn from_center_dimension<V1: IVec2, V2: IVec2>(center: V1, dimensions: V2) -> Self {
        Self::from_position_dimension(center.sub(dimensions.scaled(0.5)), dimensions)
    }

    /// creates an instance with a different position
    fn with_position<V: IVec2>(&self, position: V) -> Self {
        Self::from_position_dimension(position, self.dimensions::<V>())
    }

    /// creates the rect that fits centered on this one with the given aspect dimensions
    fn center_with_aspect_ratio<V: IVec2>(&self, aspect_dimensions: V) -> Self {
        let scale_width = self.width() / aspect_dimensions.x();
        let scale_height = self.height() / aspect_dimensions.y();

        let scale = scale_width.min(scale_height);

        let dimensions: V = aspect_dimensions.scaled(scale);

        Self::from_center_dimension(self.center::<V>(), dimensions)
    }

    /// fits the given rectangle into this one by translating it minimally
    fn fit<R: IRect>(&self, other: R) -> Self {
        type V = (f32, f32);

        let min: V = self.min();
        let max: V = self.max();
        let pos: V = other.position();
        let dim: V = other.dimensions();
        let new_pos = (
            pos.x().clamp(min.x(), max.x() - dim.x()),
            pos.y().clamp(min.y(), max.y() - dim.y()),
        );

        Self::from_position_dimension(new_pos, dim)
    }

    /// fits the given point into this one by translating it minimally
    fn fit_point<V: IVec2>(&self, point: V) -> V {
        let min: V = self.min();
        let max: V = self.max();

        V::from_xy(
            point.x().clamp(min.x(), max.x()),
            point.y().clamp(min.y(), max.y()),
        )
    }

    /// creates a rectangle with 0 area at the given point
    fn empty<V: IVec2>(position: V) -> Self {
        Self::from_position_dimension(position, V::from_xy(0.0, 0.0))
    }

    /// creates an instance based on top left and bottom right coords
    fn from_min_max<V1: IVec2, V2: IVec2>(min: V1, max: V2) -> Self {
        Self::from_position_dimension(min, max.sub(min))
    }

    /// creates a new rect by snapping this one's corners to the nearest
    fn round(&self) -> Self {
        Self::from_min_max(
            self.min::<(f32, f32)>().round(),
            self.max::<(f32, f32)>().round(),
        )
    }

    /// creates an instance from another rect
    fn from_rect<R: IRect>(other: &R) -> Self {
        Self::from_position_dimension::<(f32, f32), (f32, f32)>(other.min(), other.dimensions())
    }

    /// offsets this rectangle by the given delta
    fn offset<V: IVec2>(&self, delta: V) -> Self {
        Self::from_position_dimension(self.position::<V>().add(delta), self.dimensions::<V>())
    }

    /// creates a rectangle that contains this and another rectangle
    fn bounding<R2: IRect>(self, r2: &R2) -> Self {
        let r1_min = self.min::<(f32, f32)>();
        let r2_min = r2.min::<(f32, f32)>();
        let r1_max = self.max::<(f32, f32)>();
        let r2_max = r2.max::<(f32, f32)>();

        let min = (r1_min.x().min(r2_min.x()), r1_min.y().min(r2_min.y()));
        let max = (r1_max.x().max(r2_max.x()), r1_max.y().max(r2_max.y()));

        Self::from_min_max(min, max)
    }

    /// checks if this rectangle is empty (has 0 or negative area)
    fn is_empty(&self) -> bool {
        self.area() <= 0.0
    }

    /// gets the x position of the top-right corner
    fn x(&self) -> f32;

    /// gets they y position of the top-right corner
    fn y(&self) -> f32;

    /// gets the width
    fn width(&self) -> f32;

    /// gets the height
    fn height(&self) -> f32;

    /// gets the position of the top left corner
    fn position<V: IVec2>(&self) -> V {
        V::from_xy(self.x(), self.y())
    }

    /// alias for position or top_left
    fn min<V: IVec2>(&self) -> V {
        self.position()
    }

    /// gets the position of the bottom-right corner
    fn max<V: IVec2>(&self) -> V {
        V::from_xy(self.x() + self.width(), self.y() + self.height())
    }

    /// gets a new rect modifying x position
    fn with_x(&self, x: f32) -> Self {
        Self::from_position_dimension((x, self.y()), (self.width(), self.height()))
    }

    /// gets a new rect modifying y position
    fn with_y(&self, y: f32) -> Self {
        Self::from_position_dimension((self.x(), y), (self.width(), self.height()))
    }

    /// gets a new rect modifying center
    fn with_center<V: IVec2>(&self, center: V) -> Self {
        Self::from_center_dimension(center, self.dimensions::<V>())
    }

    /// gets a new rect modifying width
    fn with_width(&self, width: f32) -> Self {
        Self::from_position_dimension((self.x(), self.y()), (width, self.height()))
    }

    /// gets a new rect modifying height
    fn with_height(&self, height: f32) -> Self {
        Self::from_position_dimension((self.x(), self.y()), (self.width(), height))
    }

    /// gets the top left corner
    /// alias for position or min
    fn top_left<V: IVec2>(&self) -> V {
        self.position()
    }

    /// gets the top right corner
    fn top_right<V: IVec2>(&self) -> V {
        V::from_xy(self.x() + self.width(), self.y())
    }

    /// gets the bottom left corner
    fn bottom_left<V: IVec2>(&self) -> V {
        V::from_xy(self.x(), self.y() + self.height())
    }

    /// gets the bottom right corner
    /// alias for max
    fn bottom_right<V: IVec2>(&self) -> V {
        self.max()
    }

    /// gets the center point
    fn center<V: IVec2>(&self) -> V {
        let min: V = self.min();
        let max: V = self.max();

        V::from_xy(min.x().midpoint(max.x()), min.y().midpoint(max.y()))
    }

    /// gets the dimensions of the rectangle
    fn dimensions<V: IVec2>(&self) -> V {
        V::from_xy(self.width(), self.height())
    }

    /// gets the corner-to-opposite-corner length
    fn length(&self) -> f32 {
        self.dimensions::<(f32, f32)>().length()
    }

    /// checks if the rectangle is valid (width and height are non-negative)
    fn is_valid(&self) -> bool {
        self.width() >= 0.0 && self.height() > 0.0
    }

    /// gets the area
    fn area(&self) -> f32 {
        self.width() * self.height()
    }

    /// checks if a rectangle contains another rectangle
    fn contains_rect<R: IRect>(&self, other: R) -> bool {
        self.x() <= other.x()
            && other.x() + other.width() <= self.x() + self.width()
            && self.y() <= other.y()
            && other.y() + other.height() <= self.y() + self.height()
    }

    /// checks if a rectangle contains a point
    fn contains_point<V: IVec2>(&self, point: V) -> bool {
        self.x() <= point.x()
            && point.x() <= self.x() + self.width()
            && self.y() <= point.y()
            && point.y() <= self.y() + self.height()
    }

    /// checks if a rectangle intersects another rectangle
    fn intersects_rect<R: IRect>(&self, other: R) -> bool {
        self.x() < other.x() + other.width()
            && self.x() + self.width() > other.x()
            && self.y() < other.y() + other.height()
            && self.y() + self.height() > other.y()
    }

    /// constructs a new rectangle by expanding this one by the given delta
    /// the new rectangle shares the same centerpoint as this one
    fn expand(&self, delta: f32) -> Self {
        let dimensions = <(f32, f32)>::from_xy(
            (self.width() + delta * 2.0).max(0.0),
            (self.height() + delta * 2.0).max(0.0),
        );

        Self::from_center_dimension(self.center::<(f32, f32)>(), dimensions)
    }

    /// constructs a new rectangle by expanding this one's horizontal edges by the given delta
    /// the new rectangle shares the same centerpoint as this one
    fn expand_horizontal(&self, delta: f32) -> Self {
        let dimensions = ((self.width() + delta * 2.0).max(0.0), self.height());

        Self::from_center_dimension(self.center::<(f32, f32)>(), dimensions)
    }

    /// constructs a new rectangle by expanding this one's vertical edges by the given delta
    /// the new rectangle shares the same centerpoint as this one
    fn expand_vertical(&self, delta: f32) -> Self {
        let dimensions = (self.width(), (self.height() + delta * 2.0).max(0.0));

        Self::from_center_dimension(self.center::<(f32, f32)>(), dimensions)
    }

    /// constructs a new rectangle by expanding this one's top edge by the given delta
    /// prevents negative area
    fn expand_top(&self, delta: f32) -> Self {
        let delta = delta.max(-self.height());
        let position = (self.x(), self.y() - delta);
        let dimensions = (self.width(), self.height() + delta);
        Self::from_position_dimension(position, dimensions)
    }

    /// constructs a new rectangle by expanding this one's left edge by the given delta
    /// prevents negative area
    fn expand_left(&self, delta: f32) -> Self {
        let delta = delta.max(-self.width());
        let position = (self.x() - delta, self.y());
        let dimensions = (self.width() + delta, self.height());
        Self::from_position_dimension(position, dimensions)
    }

    /// constructs a new rectangle by expanding this one's right edge by the given delta
    /// prevents negative area
    fn expand_right(&self, delta: f32) -> Self {
        let dimensions = ((self.width() + delta).max(0.0), self.height());
        Self::from_position_dimension(self.position::<(f32, f32)>(), dimensions)
    }

    /// constructs a new rectangle by expanding this one's bottom edge by the given delta
    /// prevents negative area
    fn expand_bottom(&self, delta: f32) -> Self {
        let dimensions = ((self.width() + delta).max(0.0), self.height() + delta);
        Self::from_position_dimension(self.position::<(f32, f32)>(), dimensions)
    }

    /// resizes the rectangle to the given height by moving the top edge
    fn resize_top(&self, height: f32) -> Self {
        let delta = height - self.height();
        self.expand_top(delta)
    }

    /// resizes the rectangle to the given height by moving the bottom edge
    fn resize_bottom(&self, height: f32) -> Self {
        Self::from_position_dimension(
            self.position::<(f32, f32)>(),
            self.dimensions::<(f32, f32)>().with_y(height.max(0.0)),
        )
    }

    /// resizes the rectangle to the given width by moving the left edge
    fn resize_left(&self, width: f32) -> Self {
        let delta = width - self.width();
        self.expand_left(delta)
    }

    /// resizes the rectangle to the given width by moving the right edge
    fn resize_right(&self, width: f32) -> Self {
        Self::from_position_dimension(
            self.position::<(f32, f32)>(),
            self.dimensions::<(f32, f32)>().with_x(width.max(0.0)),
        )
    }

    /// evenly divides the rectangle horizontally into other rectangles
    fn divide_horizontal<const COUNT: usize>(&self, gap: f32) -> [Self; COUNT] {
        let available_width = self.width() - gap * COUNT.saturating_sub(1) as f32;
        let width = available_width / COUNT as f32;
        let increment = width + gap;

        std::array::from_fn(|i| {
            Self::from_position_dimension(
                (self.x() + increment * i as f32, self.y()),
                (width, self.height()),
            )
        })
    }

    /// evenly divides the rectangle vertically into other rectangles
    fn divide_vertical<const COUNT: usize>(&self, gap: f32) -> [Self; COUNT] {
        let available_height = self.height() - gap * COUNT.saturating_sub(1) as f32;
        let height = available_height / COUNT as f32;
        let increment = height + gap;

        std::array::from_fn(|i| {
            Self::from_position_dimension(
                (self.x(), self.y() + increment * i as f32),
                (self.width(), height),
            )
        })
    }

    /// evenly divides the rectangle into a grid of other rectangles as a matrix[row][column]
    fn divide<const ROWS: usize, const COLS: usize>(
        &self,
        x_gap: f32,
        y_gap: f32,
    ) -> [[Self; COLS]; ROWS] {
        let available_width = self.width() - x_gap * COLS.saturating_sub(1) as f32;
        let width = available_width / COLS as f32;
        let x_increment = width + x_gap;

        let available_height = self.height() - y_gap * ROWS.saturating_sub(1) as f32;
        let height = available_height / ROWS as f32;
        let y_increment = height + y_gap;

        std::array::from_fn(|row| {
            std::array::from_fn(|col| {
                Self::from_position_dimension(
                    (
                        self.x() + x_increment * col as f32,
                        self.y() + y_increment * row as f32,
                    ),
                    (width, height),
                )
            })
        })
    }

    /// divides the rectangle horizontally into other rectangles based on the given proportions
    fn divide_proportions_horizontal<const COUNT: usize, T: Copy + std::iter::Sum + AsF32>(
        &self,
        proportions: &[T; COUNT],
        gap: f32,
    ) -> [Self; COUNT] {
        let available_width = self.width() - gap * COUNT.saturating_sub(1) as f32;
        let sum: T = proportions.iter().copied().sum();
        let multiplier = available_width / sum.as_f32();
        let mut cursor = self.x();

        std::array::from_fn(|i| {
            let x = cursor;
            let width = proportions[i].as_f32() * multiplier;
            cursor += width + gap;

            Self::from_position_dimension((x, self.y()), (width, self.height()))
        })
    }

    /// divides the rectangle vertically into other rectangles based on the given proportions
    fn divide_proportions_vertical<const COUNT: usize, T: Copy + std::iter::Sum + AsF32>(
        &self,
        proportions: &[T; COUNT],
        gap: f32,
    ) -> [Self; COUNT] {
        let available_height = self.height() - gap * COUNT.saturating_sub(1) as f32;
        let sum: T = proportions.iter().copied().sum();
        let multiplier = available_height / sum.as_f32();
        let mut cursor = self.y();

        std::array::from_fn(|i| {
            let y = cursor;
            let height = proportions[i].as_f32() * multiplier;
            cursor += height + gap;

            Self::from_position_dimension((self.x(), y), (self.width(), height))
        })
    }

    /// divides the rectangle into a grid of other rectangles as a matrix[row][column] based on the
    /// given proportions
    fn divide_proportions<const ROWS: usize, const COLS: usize, T1, T2>(
        &self,
        x_proportions: &[T1; COLS],
        y_proportions: &[T2; ROWS],
        x_gap: f32,
        y_gap: f32,
    ) -> [[Self; COLS]; ROWS]
    where
        T1: Copy + std::iter::Sum + AsF32,
        T2: Copy + std::iter::Sum + AsF32,
    {
        let available_width = self.width() - x_gap * COLS.saturating_sub(1) as f32;
        let x_sum: T1 = x_proportions.iter().copied().sum();
        let x_multiplier = available_width / x_sum.as_f32();

        let available_height = self.height() - y_gap * ROWS.saturating_sub(1) as f32;
        let y_sum: T2 = y_proportions.iter().copied().sum();
        let y_multiplier = available_height / y_sum.as_f32();

        let mut y_cursor = self.y();

        std::array::from_fn(|r| {
            let y = y_cursor;
            let height = y_proportions[r].as_f32() * y_multiplier;
            y_cursor += height + y_gap;

            let mut x_cursor = self.x();
            std::array::from_fn(|c| {
                let x = x_cursor;
                let width = x_proportions[c].as_f32() * x_multiplier;
                x_cursor += width + x_gap;

                Self::from_position_dimension((x, y), (width, height))
            })
        })
    }

    /// an iterator over same-sized rectangles evenly spaced throughout this one
    /// iteration order is left to right, top to bottom
    fn evenly_spaced_rects<V: IVec2>(&self, dimensions: V) -> impl Iterator<Item = Self> {
        let col_count = (self.width() / dimensions.x()).floor() as u32;
        let row_count = (self.height() / dimensions.y()).floor() as u32;
        let cell_count = row_count * col_count;

        let x_gap = (self.width() - dimensions.x() * col_count as f32) / (col_count - 1) as f32;
        let y_gap = (self.height() - dimensions.y() * row_count as f32) / (row_count - 1) as f32;

        let mut index = 0;
        let x = self.x();
        let y = self.y();

        std::iter::from_fn(move || {
            if index >= cell_count {
                return None;
            }

            let row = index / col_count;
            let col = index % col_count;
            index += 1;

            Some(Self::from_position_dimension(
                (
                    x + col as f32 * (dimensions.x() + x_gap),
                    y + row as f32 * (dimensions.y() + y_gap),
                ),
                dimensions,
            ))
        })
    }

    /// constructs a new rectanble by moving the top left corner by the given offset
    /// prevents negative area rectangles
    fn change_min<V: IVec2>(&self, offset: V) -> Self {
        let min = self.min::<(f32, f32)>();
        let max = self.max::<(f32, f32)>();
        let new_min = (
            (min.x() + offset.x()).min(max.x()),
            (min.y() + offset.y()).min(max.y()),
        );

        Self::from_min_max(new_min, max)
    }

    /// constructs a new rectanble by moving the bottom right corner by the given offset
    /// prevents negative area rectangles
    fn change_max<V: IVec2>(&self, offset: V) -> Self {
        let min = self.min::<(f32, f32)>();
        let max = self.max::<(f32, f32)>();
        let new_max = (
            (max.x() + offset.x()).max(min.x()),
            (max.y() + offset.y()).max(min.y()),
        );

        Self::from_min_max(min, new_max)
    }
}

/// a transform combining rotation and translation, in that order
#[derive(Debug, Clone, Copy)]
pub struct RigidTransform<V: IVec2> {
    /// translation
    pub position: V,

    /// angle in RADIANS
    pub angle: f32,
}

impl<V: IVec2> Default for RigidTransform<V> {
    fn default() -> Self {
        Self {
            position: V::from_xy(0.0, 0.0),
            angle: 0.0,
        }
    }
}

impl<V: IVec2> From<V> for RigidTransform<V> {
    fn from(value: V) -> Self {
        Self {
            position: value,
            angle: 0.0,
        }
    }
}

impl<V: IVec2> RigidTransform<V> {
    /// gets the identity transform (same as defalt())
    pub fn identity() -> Self {
        Self::default()
    }

    /// apply a transform to a point
    pub fn apply_to_point<V2: IVec2>(&self, point: V2) -> V2 {
        point.rotated(self.angle).add(self.position)
    }

    /// applies the inverse of this transform to a point
    pub fn apply_inv_to_point<V2: IVec2>(&self, point: V2) -> V2 {
        point.sub(self.position).rotated(-self.angle)
    }

    /// applies this transform to another
    pub fn apply_to_other(&self, other: &Self) -> Self {
        Self {
            position: self.apply_to_point(other.position),
            angle: other.angle + self.angle,
        }
    }

    /// applies the inverse of this transform to another
    pub fn apply_inv_to_other(&self, other: &Self) -> Self {
        Self {
            position: self.apply_inv_to_point(other.position),
            angle: other.angle - self.angle,
        }
    }

    /// creates a lua table from this transform
    pub fn into_lua_table(&self, lua: &Lua) -> mlua::Result<mlua::Table> {
        let table = lua.create_table_with_capacity(0, 2)?;
        table.raw_set("x", self.position.x())?;
        table.raw_set("y", self.position.y())?;
        table.raw_set("angle", self.angle)?;
        Ok(table)
    }
}

impl<V: IVec2> FromLuaTable for RigidTransform<V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let position = finite_vec2_from_lua_table(table)?;

        let angle = match table.raw_get::<Value>("angle") {
            Ok(value) => {
                if value.is_nil() {
                    0.0
                } else {
                    match finite_f32_from_lua_value(&value) {
                        Ok(v) => v,
                        Err(msg) => return Err(format!("Invalid angle field: {msg}")),
                    }
                }
            }
            Err(msg) => return Err(format!("Invalid angle field: {msg}")),
        };

        Ok(Self { position, angle })
    }
}

impl<V: IVec2> FromLuaValue for RigidTransform<V> {
    fn try_from_lua_value(value: &mlua::prelude::LuaValue) -> Result<Self, String> {
        match value {
            Value::Table(table) => Self::try_from_lua_table(&table),
            _ => Err(format!("non-table value.")),
        }
    }
}

/// trait representing convex shapes for gjk algorithm
pub trait IConvexShape {
    /// get the furthest point in the given direction
    fn furthest_point<V: IVec2>(&self, direction: V) -> V;
}

#[derive(Debug, Clone)]
pub struct TransformedConvexShape<S: IConvexShape, V: IVec2> {
    pub shape: S,
    pub transform: RigidTransform<V>,
}

impl<V: IVec2, S: IConvexShape> TransformedConvexShape<S, V> {
    pub fn new(shape: S, transform: RigidTransform<V>) -> Self {
        Self { shape, transform }
    }
}

impl<V: IVec2, S: IConvexShape> IConvexShape for TransformedConvexShape<S, V> {
    fn furthest_point<V2: IVec2>(&self, direction: V2) -> V2 {
        let local_direction = direction.rotated(-self.transform.angle);

        let local_point = self.shape.furthest_point(local_direction);

        local_point
            .rotated(self.transform.angle)
            .add(self.transform.position)
    }
}

pub struct TransformedConvexShapeRef<'a, S: IConvexShape, V: IVec2> {
    pub shape: &'a S,
    pub transform: &'a RigidTransform<V>,
}

impl<'a, V: IVec2, S: IConvexShape> TransformedConvexShapeRef<'a, S, V> {
    pub fn new(shape: &'a S, transform: &'a RigidTransform<V>) -> Self {
        Self { shape, transform }
    }
}

impl<V: IVec2, S: IConvexShape> IConvexShape for TransformedConvexShapeRef<'_, S, V> {
    fn furthest_point<V2: IVec2>(&self, direction: V2) -> V2 {
        let local_direction = direction.rotated(-self.transform.angle);

        let local_point = self.shape.furthest_point(local_direction);

        local_point
            .rotated(self.transform.angle)
            .add(self.transform.position)
    }
}

/// a shape that can collide with others or points
#[derive(Debug, Clone)]
pub struct CollisionShape<S: Debug + IConvexShape>(pub Vec<S>);

impl<S: IConvexShape + Debug + FromLuaTable> FromLuaTable for CollisionShape<S> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        let mut convex_shapes = Vec::new();

        // try as array
        for (index, table_result) in table.sequence_values::<Table>().enumerate() {
            if let Ok(table) = table_result {
                match S::try_from_lua_table(&table) {
                    Ok(shape) => {
                        convex_shapes.push(shape);
                    }

                    Err(msg) => {
                        return Err(format!(
                            "Invalid convex shape at index {}: {msg}",
                            index + 1
                        ));
                    }
                }
            } else {
                return Err(format!(
                    "Invalid convex shape at index {}: non-table value.",
                    index + 1
                ));
            }
        }

        // try as singular shape
        if convex_shapes.is_empty() {
            convex_shapes.push(S::try_from_lua_table(table)?);
        }

        Ok(Self(convex_shapes))
    }
}

impl<S: IConvexShape + FromLuaTable + Debug> From<S> for CollisionShape<S> {
    fn from(value: S) -> Self {
        Self(vec![value])
    }
}

impl<S: IConvexShape + FromLuaTable + Debug> FromLuaValue for CollisionShape<S> {
    fn try_from_lua_value(value: &mlua::prelude::LuaValue) -> Result<Self, String> {
        match value {
            Value::Table(table) => Self::try_from_lua_table(&table),
            _ => Err(format!("non-table value.")),
        }
    }
}

impl<S: IConvexShape + Debug> Default for CollisionShape<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: IConvexShape + Debug> CollisionShape<S> {
    /// creates an empty collision shape
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// gets the furthest point in the given direction
    pub fn furthest_point<V: IVec2>(&self, direction: V) -> V {
        let mut max_dot = f32::MIN;
        let mut furthest = V::from_xy(0.0, 0.0);

        for shape in &self.0 {
            let temp = shape.furthest_point(direction);
            let temp_dot = temp.dot(direction);
            if temp_dot > max_dot {
                max_dot = temp_dot;
                furthest = temp;
            }
        }

        furthest
    }

    /// gets the furthest point in the given direction, assuming this shape has been rotated by
    /// the given angle (in radians)
    pub fn furthest_point_rotated<V: IVec2>(&self, angle: f32, direction: V) -> V {
        let new_direction = direction.rotated(angle);
        self.furthest_point(new_direction).rotated(-angle)
    }

    /// creates an aabb from this shape
    pub fn aabb<R: IRect>(&self) -> R {
        type V = (f32, f32);

        let min_x = self.furthest_point(V::from_xy(-1.0, 0.0)).x();
        let min_y = self.furthest_point(V::from_xy(0.0, -1.0)).y();
        let max_x = self.furthest_point(V::from_xy(1.0, 0.0)).x();
        let max_y = self.furthest_point(V::from_xy(0.0, 1.0)).y();

        R::from_min_max(V::from_xy(min_x, min_y), V::from_xy(max_x, max_y))
    }

    /// creates an aabb from this shape with the shape tranformed by the given rotation in radians
    pub fn aabb_rotated<R: IRect>(&self, angle: f32) -> R {
        type V = (f32, f32);

        let min_x = self
            .furthest_point_rotated(angle, V::from_xy(-1.0, 0.0))
            .x();
        let min_y = self
            .furthest_point_rotated(angle, V::from_xy(0.0, -1.0))
            .y();
        let max_x = self.furthest_point_rotated(angle, V::from_xy(1.0, 0.0)).x();
        let max_y = self.furthest_point_rotated(angle, V::from_xy(0.0, 1.0)).y();

        R::from_min_max(V::from_xy(min_x, min_y), V::from_xy(max_x, max_y))
    }

    /// creates an aabb transformed by the given transform
    pub fn aabb_transformed<V: IVec2, R: IRect>(&self, transform: &RigidTransform<V>) -> R {
        let rotated = self.aabb_rotated::<R>(transform.angle);
        R::from_position_dimension(
            rotated.position::<V>().add(transform.position),
            rotated.dimensions::<V>(),
        )
    }

    /// check if this shape contains a point
    pub fn contains_point<V: IVec2>(&self, transform: RigidTransform<V>, point: V) -> bool {
        for hull in &self.0 {
            let tshape = TransformedConvexShapeRef::new(hull, &transform);

            if gjk_intersects(&tshape, &point) {
                return true;
            }
        }
        false
    }

    /// check if this shape intersects with another, given each shape's transform
    pub fn intersects<V: IVec2>(
        &self,
        transform: &RigidTransform<V>,
        other: &Self,
        other_transform: &RigidTransform<V>,
    ) -> bool {
        for hull in &self.0 {
            for other_hull in &other.0 {
                // transform convex hulls
                let tshape = TransformedConvexShapeRef::new(hull, &transform);
                let other_tshape = TransformedConvexShapeRef::new(other_hull, &other_transform);

                // check intersection of hulls
                if gjk_intersects(&tshape, &other_tshape) {
                    return true;
                }
            }
        }

        return false;
    }
}

/// creates an aabb from a list of convex shapes
/// returns None of there are no shapes in the iterator
pub fn compute_advanced_aabb<'a, R: IRect + 'a, V: IVec2, S: IConvexShape + Debug + 'a>(
    transform: &RigidTransform<V>,
    mut shapes: impl Iterator<Item = &'a CollisionShape<S>>,
) -> Option<R> {
    let mut aabb = shapes.next()?.aabb_transformed::<V, R>(&transform);

    for shape in shapes {
        aabb = aabb.bounding(&shape.aabb_transformed::<V, R>(&transform));
    }

    Some(aabb)
}

/// uses the gjk algorithm to determine if two covexshapes are intersecting
pub fn gjk_intersects<S: IConvexShape, T: IConvexShape>(shape1: &S, shape2: &T) -> bool {
    //let mut direction = shape1.center() - shape2.center();
    let mut direction = <(f32, f32)>::from_xy(1.0, 0.0);
    let mut simplex = vec![minowski_support_2d(shape1, shape2, direction)];

    //support point for vector in direction of origin
    direction = simplex[0].normalized().neg();

    for _ in 1..=GJK_MAX_ITERATIONS {
        let point = minowski_support_2d(shape1, shape2, direction);
        if point.dot(direction) < 0.0 {
            return false;
        };
        simplex.push(point);

        if match simplex.len() {
            2 => gjk_handle_simplex_1d(&mut simplex, &mut direction),
            3 => gjk_handle_simplex_2d(&mut simplex, &mut direction),
            _ => unreachable!(),
        } {
            return true;
        };
    }
    return false;
}

//takes in a simplex and direction, then returns false
//the provided simplex must have at least two points
fn gjk_handle_simplex_1d<V: IVec2>(simplex: &mut Vec<V>, direction: &mut V) -> bool {
    let a = simplex[1];
    let b = simplex[0];
    *direction = a.origin_normal(b).normalized();
    false
}

//takes in a simplex and direction, then returns true if the point is contained in the simplex
//the provided simplex must have at least three points
fn gjk_handle_simplex_2d<V: IVec2>(simplex: &mut Vec<V>, direction: &mut V) -> bool {
    let a = simplex[2];
    let b = simplex[1];
    let c = simplex[0];

    let norm_ab = a.normal(b);
    let norm_ac = a.normal(c);
    let dir_ab = norm_ab.scaled(norm_ab.dot(a.sub(c)).signum()).normalized();
    let dir_ac = norm_ac.scaled(norm_ac.dot(a.sub(b)).signum()).normalized();

    if dir_ab.dot(a) < 0.0 {
        simplex.remove(0);
        *direction = dir_ab;
        false
    } else if dir_ac.dot(a) < 0.0 {
        simplex.remove(1);
        *direction = dir_ac;
        false
    } else {
        true
    }
}

/// gets the minowski support point in the given direction for two convex shapes
fn minowski_support_2d<V: IVec2, S: IConvexShape, T: IConvexShape>(
    s1: &S,
    s2: &T,
    direction: V,
) -> V {
    s1.furthest_point(direction)
        .sub(s2.furthest_point(direction.neg()))
}

/// a collection of points implementing IConvexShape
/// if the contained iterator is empty, the furthest point in any given direction
/// is assumed to be (0, 0)
#[derive(Debug, Clone)]
pub struct PointCloud<I: Iterator<Item = V> + Clone, V: IVec2>(pub I);

impl<I: Iterator<Item = V> + Clone, V: IVec2> IConvexShape for PointCloud<I, V> {
    fn furthest_point<V2: IVec2>(&self, direction: V2) -> V2 {
        let mut max_point = V::origin();
        let mut max_dot = max_point.dot(direction);

        for point in self.0.clone() {
            let dot = point.dot(direction);
            if dot > max_dot {
                max_dot = dot;
                max_point = point;
            }
        }
        V2::from_other(max_point)
    }
}

#[derive(Debug, Clone)]
pub struct ConvexHull<V: IVec2>(Vec<V>);

impl<V: IVec2, R: IRect> From<R> for ConvexHull<V> {
    fn from(value: R) -> Self {
        Self(vec![
            value.top_right(),
            value.top_left(),
            value.bottom_left(),
            value.bottom_right(),
        ])
    }
}

impl<V: IVec2> FromLuaTable for ConvexHull<V> {
    fn try_from_lua_table(table: &Table) -> Result<Self, String> {
        // try to parse as rect
        if let Ok(rect) = valid_finite_rect_from_lua_table::<BasicRect>(table) {
            return Ok(Self::from(rect));
        }

        // parse points from lua table
        let mut points = Vec::new();
        for (i, point_opt) in table.sequence_values::<Value>().enumerate() {
            if let Ok(point) = point_opt {
                match finite_vec2_from_lua_value(&point) {
                    Ok(p) => points.push(p),
                    Err(msg) => return Err(format!("Invalid point at index {}: {msg}", i + 1)),
                }
            } else {
                return Err(format!("Invalid point at index {}.", i + 1));
            }
        }

        if let Some(s) = Self::from_points(&mut points) {
            Ok(s)
        } else {
            Err(format!("Convex Hull must have at least 3 unique points"))
        }
    }
}

impl<V: IVec2> ConvexHull<V> {
    /// creates a convex hull of three points, uniformly distributed about the origin
    pub fn new() -> Self {
        let mut vectors = Vec::with_capacity(3);

        for i in 0..3 {
            // Distribute angles evenly between 0 and 2*PI
            let angle = (i as f32) * (2.0 * std::f32::consts::PI) / (3 as f32);
            vectors.push(V::from_xy(angle.cos(), angle.sin()));
        }

        Self(vectors)
    }

    /// attempts to create a convex hull from a list of vectors, fails if there
    /// are less than 3 points
    pub fn from_points(points: &mut [V]) -> Option<Self> {
        Some(Self(monotone_convex_hull(points)?))
    }

    /// returns the list of points
    pub fn points(&self) -> &[V] {
        &self.0
    }

    /// inserts a point into this hull
    pub fn insert(&mut self, point: V) {
        self.0.push(point);
        self.0 = monotone_convex_hull(&mut self.0).unwrap();
    }
}

impl<V: IVec2> IConvexShape for ConvexHull<V> {
    fn furthest_point<V2: IVec2>(&self, direction: V2) -> V2 {
        let mut max_point = self.0[0];
        let mut max_dot = max_point.dot(direction);

        for point in self.points().iter().skip(1) {
            let dot = point.dot(direction);
            if dot > max_dot {
                max_dot = dot;
                max_point = *point;
            }
        }
        V2::from_other(max_point)
    }
}

/// computes the convex hull of a list of vec2s using andrew's monotone chain algorithm
/// the returned points will be in counterclockwise order
/// may fail if the following conditions are not met:
///  - all points are finite
///  - there are at least 3 unique points
pub fn monotone_convex_hull<V: IVec2>(points: &mut [V]) -> Option<Vec<V>> {
    if points.len() < 3 {
        return None;
    }

    // sort points by x, then y
    points.sort_unstable_by(|a, b| {
        let order = a
            .x()
            .partial_cmp(&b.x())
            .expect("Points passed to convex hull algorithm should be finite.");
        if order == Ordering::Equal {
            a.y()
                .partial_cmp(&b.y())
                .expect("Points passed to convex hull algorithm should be finite.")
        } else {
            order
        }
    });

    // build lower hull
    let mut lower: Vec<V> = Vec::new();
    for p in points.iter() {
        while lower.len() >= 2 && lower[lower.len() - 2].cross(*lower.last().unwrap(), *p) <= 0.0 {
            lower.pop();
        }
        lower.push(p.clone());
    }

    // build upper hull
    let mut upper: Vec<V> = Vec::new();
    for p in points.iter().rev() {
        while upper.len() >= 2 && upper[upper.len() - 2].cross(*upper.last().unwrap(), *p) <= 0.0 {
            upper.pop();
        }
        upper.push(p.clone());
    }

    // combine and return hulls
    lower.pop();
    upper.pop();
    lower.append(&mut upper);

    if lower.len() < 3 { None } else { Some(lower) }
}
