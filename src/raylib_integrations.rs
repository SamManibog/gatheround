use std::{collections::BTreeMap, path::PathBuf};

use raylib::{
    RaylibHandle, RaylibThread,
    drawing::{RaylibDraw, RaylibScissorModeExt, RaylibTextureModeExt},
    ffi::{Color, Rectangle, Vector2},
    text::{Font, RaylibFont},
    texture::{Image, RaylibTexture2D, RenderTexture2D, Texture2D},
};

use crate::{
    color::IColor,
    geometry::{IRect, IVec2},
    piece_components::{
        Alignment, ImageTransform, PatternInfo, PatternType, TextboxInfo, TextureInfo,
        VisualComponent, VisualInfo,
    },
    renderer::IRenderer,
    resources::VisualComponentProvider,
};

mod uiconsts {
    use raylib::ffi::Color;

    pub const BORDER_THICKNESS: f32 = 2.0;
    pub const CONTENT_GAP: f32 = 4.0;
    pub const BUTTON_PADDING: f32 = 2.0;
    pub const TITLE_HEIGHT: f32 = 32.0;
    pub const TITLE_FONT_SIZE: f32 = 30.0;
    pub const FONT_SIZE: f32 = 20.0;
    pub const PROPERTIES_PANEL_WIDTH: f32 = 400.0;

    pub const TEXT_COLOR: Color = Color::BLACK;
    pub const DISABLED_TEXT_COLOR: Color = Color::LIGHTGRAY;

    pub const ENABLED_BUTTON_COLOR: Color = Color::LIGHTBLUE;
    pub const DISABLED_BUTTON_COLOR: Color = Color::GRAY;
    pub const HOVERED_BUTTON_COLOR: Color = Color::LIGHTSKYBLUE;
    pub const CLICKED_BUTTON_COLOR: Color = Color::LIGHTSALMON;

    pub const DEFAULT_FONT_PATH: &str = r"C:\Windows\Fonts\arial.ttf";
}

impl IVec2 for Vector2 {
    fn from_xy(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn x(&self) -> f32 {
        self.x
    }

    fn y(&self) -> f32 {
        self.y
    }
}

impl IRect for Rectangle {
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
        Rectangle {
            x: position.x(),
            y: position.y(),
            width: dimensions.x(),
            height: dimensions.y(),
        }
    }
}

impl IColor for Color {
    fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    fn r(&self) -> u8 {
        self.r
    }

    fn g(&self) -> u8 {
        self.g
    }

    fn b(&self) -> u8 {
        self.b
    }

    fn a(&self) -> u8 {
        self.a
    }
}

pub trait FromRenderTexture {
    fn from_render_texture(render_texture: RenderTexture2D) -> Self;
}

impl FromRenderTexture for RenderTexture2D {
    fn from_render_texture(render_texture: RenderTexture2D) -> Self {
        render_texture
    }
}

impl FromRenderTexture for Texture2D {
    fn from_render_texture(render_texture: RenderTexture2D) -> Self {
        unsafe { Texture2D::from_raw(render_texture.texture) }
    }
}

impl FromRenderTexture for Image {
    fn from_render_texture(render_texture: RenderTexture2D) -> Self {
        render_texture.load_image().unwrap()
    }
}

pub struct RaylibRenderer<'a> {
    pub rl: &'a mut RaylibHandle,
    pub thread: &'a RaylibThread,
}

impl<'a> RaylibRenderer<'a> {
    const TEXTURE_PADDING: f32 = 2.0;
    const MIN_AUTO_FONT_SIZE: f32 = 4.0;
    const MIN_AUTO_LINE_SPACING: f32 = 1.05;
    const MAX_PATTERN_OFFSET: f32 = 50.0;

    pub fn from_handle(rl: &'a mut RaylibHandle, thread: &'a RaylibThread) -> Self {
        Self { rl, thread }
    }

    pub fn load_image(&mut self, path: PathBuf) -> Option<Texture2D> {
        let string = path.to_str()?.to_owned();

        if let Ok(tex) = self.rl.load_texture(&self.thread, &string) {
            Some(tex)
        } else {
            None
        }
    }

    pub fn get_fitting_text_width(font: &Font, text: &str, max_line_length: f32) -> f32 {
        let base_size = 32.0;
        let actual_size = font.measure_text(text, base_size, 0.0).x;

        let size_ratio = max_line_length / actual_size;

        base_size * size_ratio
    }

    pub fn draw_texture(
        &mut self,
        transform: &ImageTransform<Vector2>,
        texture: &Texture2D,
        target: &mut RenderTexture2D,
    ) {
        let mut painter = self.rl.begin_texture_mode(&self.thread, target);
        painter.draw_texture_ex(
            texture,
            transform.offset,
            0.0,
            transform.scale,
            Color::WHITE,
        );
    }

    pub fn draw_textbox<C: IColor, R: IRect>(
        &mut self,
        textbox: &TextboxInfo<C, R>,
        visual_component_provider: &mut VisualComponentProvider<'_, Font, Texture2D>,
        transform: &ImageTransform<Vector2>,
        target: &mut RenderTexture2D,
    ) {
        // attempt to get the font, return if font could not be found
        let font = if let Some(f) = visual_component_provider.get_or_load_font(
            self,
            &textbox.font_family,
            textbox.font_size.unwrap_or(32),
            &textbox.text,
        ) {
            f
        } else {
            return;
        };

        // the true draw rectangle
        let rect: Rectangle = transform.transform_rect(textbox.rect);

        // break text into lines
        let lines: Vec<&str> = textbox.text.split("\n").collect();

        // determine font size and line spacing
        // both values are given as a size in pixels
        // uses the following equations:
        // let t = total
        // let s = spacing
        // let f = font size
        // let c = line count
        //
        // t = cf + sf(c - 1)
        //
        // t = cf + sf(c - 1)
        // t = f(c + s(c-1))
        // t = f(c + sc - s)
        // f = t / (c + sc - s)
        // f = t / (c(1 + s) - s)
        //
        // t = cf + sf(c - 1)
        // sf(c - 1) = t - cf
        // s = (t - cf)/(f(c - 1))
        // s = (t - cf)/(fc - f)
        let (font_size, line_increment) = match (textbox.font_size, textbox.line_spacing) {
            (None, None) => {
                // determine maximum size based on textbox width
                let max_size = Self::get_fitting_text_width(&font, &textbox.text, rect.x())
                    .max(Self::MIN_AUTO_FONT_SIZE);

                // determine line spacing for max size
                let total_space = rect.height() - max_size * lines.len() as f32;
                let max_size_increment =
                    (total_space / (lines.len().saturating_sub(1)) as f32) + max_size;

                if max_size_increment < max_size * Self::MIN_AUTO_LINE_SPACING {
                    // text needs to be resized
                    let font_size = (rect.height()
                        / (lines.len() as f32 * (1.0 + Self::MIN_AUTO_LINE_SPACING)
                            - Self::MIN_AUTO_LINE_SPACING))
                        .max(Self::MIN_AUTO_FONT_SIZE);

                    (
                        font_size.floor(),
                        (Self::MIN_AUTO_LINE_SPACING * font_size).floor(),
                    )
                } else {
                    // fits with additional spacing
                    (max_size.floor(), max_size_increment.floor())
                }
            }
            (Some(size), None) => {
                let size = size as f32 * transform.scale;

                let font_space = size * lines.len() as f32;
                let spacing = ((rect.height() - font_space) / (font_space - size))
                    .max(Self::MIN_AUTO_LINE_SPACING);
                (size.floor(), (spacing * size).floor())
            }
            (None, Some(spacing)) => {
                // determine maximum size based on textbox width
                let width_sized = Self::get_fitting_text_width(&font, &textbox.text, rect.x());

                // determine maximum sized based on textbox height
                let height_sized = rect.height() / (lines.len() as f32 * (1.0 + spacing) - spacing);

                // take the minimum of both sizes
                let size = width_sized.min(height_sized).max(Self::MIN_AUTO_FONT_SIZE);

                (size.floor(), (spacing * size).floor())
            }
            (Some(size), Some(spacing)) => {
                let size = size as f32 * transform.scale;

                (size.floor(), (spacing * size).floor())
            }
        };

        // load font in correct size
        let font = if let Some(f) = visual_component_provider.get_or_load_font(
            self,
            &textbox.font_family,
            textbox.font_size.unwrap_or(32),
            &textbox.text,
        ) {
            f
        } else {
            return;
        };

        // determine starting y position
        let text_height = font_size + line_increment * (lines.len() - 1) as f32;
        let mut y_cursor = match textbox.align.vertical_align() {
            Alignment::Begin => rect.y,
            Alignment::Center => (rect.height - text_height) / 2.0 + rect.y,
            Alignment::End => rect.max::<(f32, f32)>().y() - text_height,
        };

        // draw textbox
        let color = Color::from_other(&textbox.color);
        match textbox.align.horizontal_align() {
            Alignment::Begin => {
                let mut painter = self.rl.begin_texture_mode(&self.thread, target);
                for line in lines {
                    let position = Vector2::from_xy(rect.x, y_cursor);
                    painter.draw_text_ex(&font, line, position, font_size, 0.0, color);

                    y_cursor += line_increment;
                }
            }

            Alignment::Center => {
                let mut painter = self.rl.begin_texture_mode(&self.thread, target);
                for line in lines {
                    let line_length = font.measure_text(&textbox.text, font_size, 0.0).x;

                    let position =
                        Vector2::from_xy((rect.width - line_length) / 2.0 + rect.x, y_cursor);
                    painter.draw_text_ex(&font, line, position, font_size, 0.0, color);

                    y_cursor += line_increment;
                }
            }

            Alignment::End => {
                let mut painter = self.rl.begin_texture_mode(&self.thread, target);
                for line in lines {
                    let line_length = font.measure_text(&textbox.text, font_size, 0.0).x;

                    let position = Vector2::from_xy(rect.width - line_length + rect.x, y_cursor);
                    painter.draw_text_ex(&font, line, position, font_size, 0.0, color);

                    y_cursor += line_increment;
                }
            }
        }
    }

    pub fn draw_pattern<C: IColor, R: IRect, V: IVec2>(
        &mut self,
        pattern: &PatternInfo<C, R, V>,
        transform: &ImageTransform<Vector2>,
        target: &mut RenderTexture2D,
    ) {
        let rect: Rectangle = transform.transform_rect(pattern.rect);

        let pattern_texture = match pattern.pattern {
            PatternType::Solid => {
                self.draw_solid_pattern(&pattern, &transform, target);
                return;
            }

            PatternType::Checkers => self.make_checkers(&pattern, &transform),
            PatternType::Stripes => self.make_stripes(&pattern, &transform),
            PatternType::Dots => self.make_dots(&pattern, &transform),
        };
        if pattern_texture.is_none() {
            return;
        }
        let pattern_texture = pattern_texture.unwrap();

        let mut painter = self.rl.begin_texture_mode(&self.thread, target);
        let mut painter = painter.begin_scissor_mode(
            rect.x.round() as i32,
            rect.y.round() as i32,
            rect.width.round() as i32,
            rect.height.round() as i32,
        );

        let pattern_dimensions = Vector2::from_xy(
            pattern_texture.width() as f32,
            pattern_texture.height() as f32,
        );

        let offset = Vector2::from_xy(
            pattern
                .offset
                .x()
                .clamp(-Self::MAX_PATTERN_OFFSET, Self::MAX_PATTERN_OFFSET),
            pattern
                .offset
                .y()
                .clamp(-Self::MAX_PATTERN_OFFSET, Self::MAX_PATTERN_OFFSET),
        );

        let src_rect = Rectangle::from_center_dimension(
            pattern_dimensions.scaled(0.5).add(offset),
            Vector2::from_xy(1.0, 1.0).scaled(rect.length()),
        );

        painter.draw_texture_pro(
            pattern_texture,
            src_rect,
            Rectangle::from_center_dimension(
                rect.center::<Vector2>(),
                src_rect.dimensions::<Vector2>(),
            ),
            rect.center::<Vector2>(),
            pattern.angle,
            Color::WHITE,
        );
    }

    pub fn draw_solid_pattern<C: IColor, R: IRect, V: IVec2>(
        &mut self,
        pattern: &PatternInfo<C, R, V>,
        transform: &ImageTransform<Vector2>,
        target: &mut RenderTexture2D,
    ) {
        let rect: Rectangle = transform.transform_rect(pattern.rect);
        let mut hidden_painter = self.rl.begin_texture_mode(&self.thread, target);
        let mut painter = hidden_painter.begin_scissor_mode(
            rect.x.round() as i32,
            rect.y.round() as i32,
            rect.width.round() as i32,
            rect.height.round() as i32,
        );

        let color = Color::from_other(&pattern.pattern_color);

        painter.draw_rectangle_rec(rect, color);
    }

    pub fn make_checkers<C: IColor, R: IRect, V: IVec2>(
        &mut self,
        pattern: &PatternInfo<C, R, V>,
        transform: &ImageTransform<Vector2>,
    ) -> Option<RenderTexture2D> {
        let rect: Rectangle = transform.transform_rect(pattern.rect);

        // determine actual values
        let weight = pattern.weight * transform.scale;
        let spacing = pattern.spacing * transform.scale;
        let increment = weight + spacing;

        // create the pattern texture
        let pattern_side_length =
            (rect.dimensions::<Vector2>().length() + Self::MAX_PATTERN_OFFSET * 2.0).ceil() as u32;
        let mut pattern_texture = match self.rl.load_render_texture(
            &self.thread,
            pattern_side_length,
            pattern_side_length,
        ) {
            Ok(v) => v,
            Err(_) => {
                return None;
            }
        };

        let mut painter = self
            .rl
            .begin_texture_mode(&self.thread, &mut pattern_texture);

        // make the background
        let background = Color::from_other(&pattern.background_color);
        painter.draw_rectangle(
            0,
            0,
            pattern_side_length as i32,
            pattern_side_length as i32,
            background,
        );

        // make the foreground
        let color = Color::from_other(&pattern.pattern_color);

        // draw individual checkers
        let mut x_cursor = 0.0;
        let square_count = (pattern_side_length as f32 / (increment) + 1.0).ceil() as u32 + 1;
        for x in 0..square_count {
            let mut y_cursor = 0.0;

            for y in 0..square_count {
                if (x + y) % 2 == 0 {
                    let r = Rectangle {
                        x: x_cursor,
                        y: y_cursor,
                        width: weight,
                        height: weight,
                    };

                    painter.draw_rectangle_rec(r, color);
                }
                y_cursor += increment;
            }
            x_cursor += increment;
        }

        drop(painter);

        Some(pattern_texture)
    }

    pub fn make_stripes<C: IColor, R: IRect, V: IVec2>(
        &mut self,
        pattern: &PatternInfo<C, R, V>,
        transform: &ImageTransform<Vector2>,
    ) -> Option<RenderTexture2D> {
        let rect: Rectangle = transform.transform_rect(pattern.rect);

        // determine actual values
        let weight = pattern.weight * transform.scale;
        let spacing = pattern.spacing * transform.scale;
        let increment = weight + spacing;

        // create the pattern texture
        let pattern_side_length =
            (rect.dimensions::<Vector2>().length() + Self::MAX_PATTERN_OFFSET * 2.0).ceil() as u32;
        let mut pattern_texture = match self.rl.load_render_texture(
            &self.thread,
            pattern_side_length,
            pattern_side_length,
        ) {
            Ok(v) => v,
            Err(_) => {
                return None;
            }
        };
        let height = pattern_texture.height() as f32;

        let mut painter = self
            .rl
            .begin_texture_mode(&self.thread, &mut pattern_texture);

        // make the background
        let background = Color::from_other(&pattern.background_color);
        painter.draw_rectangle(
            0,
            0,
            pattern_side_length as i32,
            pattern_side_length as i32,
            background,
        );

        // make the foreground
        let color = Color::from_other(&pattern.pattern_color);

        // draw individual checkers
        let mut cursor = 0.0;
        let stripe_count = (pattern_side_length as f32 / (increment) + 1.0).ceil() as u32 + 1;
        for x in 0..stripe_count {
            if x % 2 == 0 {
                let r = Rectangle {
                    x: cursor,
                    y: 0.0,
                    width: weight,
                    height,
                };

                painter.draw_rectangle_rec(r, color);
            }
            cursor += increment;
        }

        drop(painter);

        Some(pattern_texture)
    }

    pub fn make_dots<C: IColor, R: IRect, V: IVec2>(
        &mut self,
        pattern: &PatternInfo<C, R, V>,
        transform: &ImageTransform<Vector2>,
    ) -> Option<RenderTexture2D> {
        let rect: Rectangle = transform.transform_rect(pattern.rect);

        // determine actual values
        let weight = pattern.weight * transform.scale;
        let spacing = pattern.spacing * transform.scale;
        let increment = weight + spacing;

        // create the pattern texture
        let pattern_side_length =
            (rect.dimensions::<Vector2>().length() + Self::MAX_PATTERN_OFFSET * 2.0).ceil() as u32;
        let mut pattern_texture = match self.rl.load_render_texture(
            &self.thread,
            pattern_side_length,
            pattern_side_length,
        ) {
            Ok(v) => v,
            Err(_) => {
                return None;
            }
        };

        let mut painter = self
            .rl
            .begin_texture_mode(&self.thread, &mut pattern_texture);

        // make the background
        let background = Color::from_other(&pattern.background_color);
        painter.draw_rectangle(
            0,
            0,
            pattern_side_length as i32,
            pattern_side_length as i32,
            background,
        );

        // make the foreground
        let color = Color::from_other(&pattern.pattern_color);

        // draw individual checkers
        let radius = weight / 2.0;
        let mut x_cursor = radius;
        let dot_count = (pattern_side_length as f32 / (increment) + 1.0).ceil() as u32 + 1;
        for _ in 0..dot_count {
            let mut y_cursor = radius;

            for _ in 0..dot_count {
                painter.draw_circle(
                    x_cursor.round() as i32,
                    y_cursor.round() as i32,
                    radius,
                    color,
                );

                y_cursor += increment;
            }
            x_cursor += increment;
        }

        drop(painter);

        Some(pattern_texture)
    }
}

impl<'a> IRenderer for RaylibRenderer<'a> {
    type Texture = Texture2D;
    type Font = Font;

    fn compile_visual<C, R, V>(
        &mut self,
        visual: &VisualInfo<C, R, V>,
        magnification_dimensions: V,
        mut visual_component_provider: VisualComponentProvider<'_, Self::Font, Self::Texture>,
    ) -> Result<TextureInfo<Self::Texture>, String>
    where
        C: IColor,
        R: IRect,
        V: IVec2,
    {
        // determine the bounds for the original image
        let render_dimensions = {
            // compute the base rect
            let mut rect = Rectangle::new(0.0, 0.0, 0.0, 0.0);
            for component in visual.components() {
                match component {
                    VisualComponent::Textbox(t) => {
                        rect = rect.bounding(&t.rect);
                    }

                    VisualComponent::Image(i) => {
                        if let Some(image_texture) =
                            visual_component_provider.get_or_load_image(self, &i.path)
                        {
                            let r = Rectangle::from_position_dimension(
                                i.transform.offset,
                                Vector2::from_xy(
                                    image_texture.width as f32,
                                    image_texture.height as f32,
                                ),
                            );

                            rect = rect.bounding(&r);
                        }
                    }

                    VisualComponent::Pattern(p) => rect = rect.bounding(&p.rect),
                }
            }

            // dimensions are a square centered on the origin, containing all components
            // and some padding
            let min: (f32, f32) = rect.min();
            let max: (f32, f32) = rect.max();
            let x = (min.0.abs().max(max.0.abs()) + Self::TEXTURE_PADDING) * 2.0;
            let y = (min.1.abs().max(max.1.abs()) + Self::TEXTURE_PADDING) * 2.0;

            Vector2::new(x, y)
        };

        // create a render texture with the relevant bounds
        let width = render_dimensions.x.ceil() as u32;
        let height = render_dimensions.x.ceil() as u32;

        let mut texture = self
            .rl
            .load_render_texture(&self.thread, width, height)
            .expect("Could not create render texture.");

        let texture_transform = ImageTransform::<Vector2> {
            offset: Vector2::from_xy(width as f32, height as f32).scale(0.5),
            scale: 1.0,
        };

        // create the magnified texture
        let magnification = {
            let x_scale = magnification_dimensions.x() / render_dimensions.x.min(1.0);
            let y_scale = magnification_dimensions.y() / render_dimensions.y.min(1.0);

            // take the smallest of the scale factors to ensure fit
            x_scale.min(y_scale)
        };

        let mangnified_width = (render_dimensions.x * magnification).ceil() as u32;
        let magnified_height = (render_dimensions.y * magnification).ceil() as u32;

        let mut texture_magnified = self
            .rl
            .load_render_texture(&self.thread, mangnified_width, magnified_height)
            .expect("Could not create magnified render texture.");

        let magnified_transform = ImageTransform::<Vector2> {
            offset: Vector2::from_xy(mangnified_width as f32, magnified_height as f32).scale(0.5),
            scale: magnification,
        };

        // draw visuals onto the render texture
        for component in visual.components() {
            match component {
                VisualComponent::Textbox(t) => {
                    self.draw_textbox(
                        t,
                        &mut visual_component_provider,
                        &texture_transform,
                        &mut texture,
                    );
                    self.draw_textbox(
                        t,
                        &mut visual_component_provider,
                        &magnified_transform,
                        &mut texture_magnified,
                    );
                }

                VisualComponent::Image(i) => {
                    if let Some(image) = visual_component_provider.get_or_load_image(self, &i.path)
                    {
                        self.draw_texture(
                            &texture_transform.transform_other(&i.transform),
                            &image,
                            &mut texture,
                        );
                        self.draw_texture(
                            &magnified_transform.transform_other(&i.transform),
                            &image,
                            &mut texture_magnified,
                        );
                    }
                }

                VisualComponent::Pattern(p) => {
                    self.draw_pattern(p, &texture_transform, &mut texture);
                    self.draw_pattern(p, &magnified_transform, &mut texture_magnified);
                }
            }
        }

        Ok(TextureInfo {
            texture: unsafe { Texture2D::from_raw(texture.to_raw().texture) },
            texture_magnified: unsafe { Texture2D::from_raw(texture_magnified.to_raw().texture) },
        })
    }

    fn load_font(
        &mut self,
        path: &std::path::Path,
        size: u32,
        characters: &str,
    ) -> Result<Self::Font, String> {
        match self.rl.load_font_ex(
            &self.thread,
            &path.to_string_lossy(),
            size as i32,
            Some(characters),
        ) {
            Ok(f) => Ok(f),
            Err(_) => Err(format!(
                "Failed to load font from '{}'",
                path.to_string_lossy()
            )),
        }
    }

    fn load_image(&mut self, path: &std::path::Path) -> Result<Texture2D, String> {
        match self.rl.load_texture(&self.thread, &path.to_string_lossy()) {
            Ok(tex) => Ok(tex),

            Err(err) => match err {
                raylib::error::LoadTextureError::TextureFromFileFailed { path } => Err(format!(
                    "Could not load image from '{}'. Either file does not exist or the format is not supported.",
                    path
                )),

                _ => Err(format!(
                    "Failed to load image from '{}'",
                    path.to_string_lossy()
                )),
            },
        }
    }

    fn draw_board<
        R: IRect,
        V: IVec2,
        S: crate::geometry::IConvexShape + crate::lua::FromLuaTable,
    >(
        &mut self,
        board: &crate::board::BoardManager<R, V, S, Self::Texture, Self::Font>,
    ) {
        let draw_layers = BTreeMap::from_iter(board.draw_layer_iter().map(|l| {
            (
                l,
                self.rl.load_render_texture(
                    self.thread,
                    self.rl.get_render_width() as u32,
                    self.rl.get_render_height() as u32,
                ),
            )
        }));

        erroror lsofsldkfjsfjl

        todo!()
    }
}

/*
struct FocusedVisual {
    entity: VisualEntity,
    focused_component: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Released,
    JustReleased,
    Pressed,
    JustPressed,
}

pub struct MouseButtonState {
    // the current position of the mouse pointer
    pub position: Vector2,

    // the position of the mouse pointer when state last changed
    pub action_position: Vector2,

    // the current state of the mouse pointer
    pub state: KeyState,

    // the mouse button to check
    pub button: MouseButton,
}

impl MouseButtonState {
    pub fn new(button: MouseButton) -> Self {
        Self {
            position: Vector2::ZERO,
            action_position: Vector2::ZERO,
            state: KeyState::Released,
            button,
        }
    }

    pub fn update(&mut self, rl: &RaylibHandle) {
        self.position = rl.get_mouse_position();

        if self.state == KeyState::JustPressed {
            self.state = KeyState::Pressed;
        }
        if self.state == KeyState::JustReleased {
            self.state = KeyState::Released;
        }

        if rl.is_mouse_button_down(self.button) {
            if self.is_released() {
                self.state = KeyState::JustPressed;
                self.action_position = self.position;
            }
        } else {
            if self.is_pressed() {
                self.state = KeyState::JustReleased;
                self.action_position = self.position;
            }
        }
    }

    pub fn is_just_pressed(&self) -> bool {
        self.state == KeyState::JustPressed
    }

    pub fn is_pressed(&self) -> bool {
        self.state == KeyState::Pressed || self.state == KeyState::JustPressed
    }

    pub fn is_just_released(&self) -> bool {
        self.state == KeyState::JustReleased
    }

    pub fn is_released(&self) -> bool {
        self.state == KeyState::Released || self.state == KeyState::JustReleased
    }

    pub fn position(&self) -> Vector2 {
        self.position
    }

    pub fn action_position(&self) -> Vector2 {
        self.action_position
    }
}

struct InterfaceState<'a> {
    painter: RaylibDrawHandle<'a>,
    lmb_state: &'a mut MouseButtonState,
    title_font: &'a Font,
    font: &'a Font,
}

impl<'a> InterfaceState<'a> {
    /// draw a clickable text button returns true if the button was just clicked
    fn draw_text_button(
        &mut self,
        rect: Rectangle,
        text: &str,
        enabled: bool,
    ) -> bool {
        let mut output = false;
        let text_width = self.font.measure_text(text, uiconsts::FONT_SIZE, 0.0).x;
        let text_rect = Rectangle::from_center_dimension(
            rect.center::<Vector2>(),
            (text_width, uiconsts::FONT_SIZE),
        );
        self.painter.draw_scissor_mode(
            rect.x as i32,
            rect.y as i32,
            rect.width as i32,
            rect.height as i32,
            |mut painter| {
                if enabled {
                    if rect.contains_point(painter.get_mouse_position()) {
                        if self.lmb_state.is_just_pressed()
                        {
                            output = true;
                            painter.draw_rectangle_rec(rect, uiconsts::CLICKED_BUTTON_COLOR);
                        } else {
                            painter.draw_rectangle_rec(rect, uiconsts::HOVERED_BUTTON_COLOR);
                        }
                    } else {
                        painter.draw_rectangle_rec(rect, uiconsts::ENABLED_BUTTON_COLOR);
                    }
                } else {
                    painter.draw_rectangle_rec(rect, uiconsts::DISABLED_BUTTON_COLOR);
                }

                let color = if enabled {
                    uiconsts::TEXT_COLOR
                } else {
                    uiconsts::DISABLED_TEXT_COLOR
                };

                painter.draw_text_ex(
                    self.font,
                    text,
                    text_rect.min::<Vector2>(),
                    uiconsts::FONT_SIZE,
                    0.0,
                    color,
                )
            },
        );

        output
    }

    /// draw vertically-centered text to the given rect
    fn draw_title(&mut self, rect: Rectangle, text: &str) {
        let y = rect.center::<Vector2>().y - uiconsts::TITLE_FONT_SIZE / 2.0;
        self.painter.draw_scissor_mode(
            rect.x as i32,
            rect.y as i32,
            rect.width as i32,
            rect.height as i32,
            |mut painter| {
                painter.draw_text_ex(
                    &self.title_font,
                    text,
                    Vector2::from_xy(rect.x, y),
                    uiconsts::TITLE_FONT_SIZE,
                    0.0,
                    uiconsts::TEXT_COLOR,
                )
            },
        );
    }

    /// draw top-left aligned text to the given rect
    fn draw_text(&mut self, rect: Rectangle, text: &str) {
        self.painter.draw_scissor_mode(
            rect.x as i32,
            rect.y as i32,
            rect.width as i32,
            rect.height as i32,
            |mut painter| {
                painter.draw_text_ex(
                    self.font,
                    text,
                    Vector2::from_xy(rect.x, rect.y),
                    uiconsts::FONT_SIZE,
                    0.0,
                    uiconsts::TEXT_COLOR,
                )
            },
        );
    }
}

/// raylib implementation of project editor
pub struct RaylibEditorState {
    /// "savable" components for the current project
    save_state: EditorSaveState<
        Color,
        Rectangle,
        Vector2,
        ConvexHull<Vector2>,
        Texture2D,
        PathBuf,
        Texture2D,
    >,

    /// compiled visuals for later use
    compiled_visuals: DenseComponentManager<VisualEntity, Texture2D>,

    /// focused visual
    focused_visual: Option<FocusedVisual>,

    /// visual transform
    visual_transform: ImageTransform<Vector2>,
}

impl RaylibEditorState {
    fn new(project_directory: PathBuf) -> Self {
        let save_state = EditorSaveState::new("Test".to_owned(), project_directory);
        Self {
            save_state,
            focused_visual: None,
            compiled_visuals: DenseComponentManager::new(),
            visual_transform: ImageTransform::identity(),
        }
    }

    fn update(&mut self, mut interface: &mut InterfaceState) {
        interface.painter.clear_background(Color::WHITE);
        self.draw_visual(&mut interface);
        self.draw_ui(&mut interface);
    }

    fn draw_visual(&self, interface: &mut InterfaceState) {
        let texture = if let Some(vis) = &self.focused_visual {
            if let Some(tex) = self.compiled_visuals.get(vis.entity) {
                tex
            } else {
                return;
            }
        } else {
            return;
        };

        interface.painter.draw_texture_ex(
            texture,
            self.visual_transform.offset,
            0.0,
            self.visual_transform.scale,
            Color::WHITE,
        );
    }

    fn draw_ui(&mut self, interface: &mut InterfaceState) {
        let right_panel_rect = Rectangle {
            x: interface.painter.get_screen_width() as f32 - uiconsts::PROPERTIES_PANEL_WIDTH,
            y: 0.0,
            width: uiconsts::PROPERTIES_PANEL_WIDTH,
            height: interface.painter.get_screen_height() as f32,
        };
        self.draw_visual_editor_panel(interface, right_panel_rect);
    }

    fn draw_visual_editor_panel(&mut self, interface: &mut InterfaceState, rect: Rectangle) {
        interface
            .painter
            .draw_rectangle_rec(rect.clone(), Color::RAYWHITE);
        interface.painter.draw_line_ex(
            rect.top_left::<Vector2>(),
            rect.bottom_left::<Vector2>(),
            uiconsts::BORDER_THICKNESS,
            Color::GRAY,
        );

        let content_rect: Rectangle = rect.expand(-uiconsts::CONTENT_GAP);
        let [properties_rect, inspector_rect] =
            content_rect.divide_proportions_vertical(&[2, 1], uiconsts::CONTENT_GAP);

        self.draw_property_editor(interface, properties_rect);
        self.draw_inspector(interface, inspector_rect);
    }

    fn draw_property_editor(&mut self, interface: &mut InterfaceState, rect: Rectangle) {
        let title_rect = rect.resize_bottom(uiconsts::TITLE_HEIGHT);
        let content_rect = rect.expand_top(-uiconsts::TITLE_HEIGHT - uiconsts::CONTENT_GAP);
        let name_rect = content_rect.resize_bottom(uiconsts::FONT_SIZE);
        let properties_rect = content_rect.expand_top(-name_rect.height - uiconsts::CONTENT_GAP);

        interface.draw_title(title_rect, "Properties");

        if let Some(vis) = &mut self.focused_visual
            && self
                .save_state
                .resources
                .visuals
                .contains_visual(vis.entity)
        {
            let visual = self
                .save_state
                .resources
                .visuals
                .get_visual(vis.entity)
                .unwrap();
            vis.focused_component = vis.focused_component.min(visual.len().saturating_sub(1));

            if let Some(component_name) = visual.get_name(vis.focused_component) {
                interface.draw_text(
                    name_rect,
                    &format!(
                        "{}: {}",
                        self.save_state
                            .resources
                            .visuals
                            .get_visual_name(vis.entity)
                            .unwrap(),
                        component_name
                    ),
                );
            } else {
                interface.draw_text(
                    name_rect,
                    &self
                        .save_state
                        .resources
                        .visuals
                        .get_visual_name(vis.entity)
                        .unwrap(),
                );
                return;
            }

            self.save_state
                .resources
                .visuals
                .partial_modify_visual_component(vis.entity, vis.focused_component, |component| {
                    match component {
                        VisualComponentHandle::Textbox(t) => {
                            Self::draw_textbox_editor(t, interface, properties_rect);
                        }
                        VisualComponentHandle::Pattern(p) => {
                            Self::draw_pattern_editor(p, interface, properties_rect);
                        }
                        VisualComponentHandle::Image(i) => {
                            Self::draw_image_editor(i, interface, properties_rect);
                        }
                    }
                });
        } else {
            self.focused_visual = None;
            interface.draw_text(name_rect, "Pick a visual from the left panel.");
        }
    }

    fn draw_textbox_editor(
        textbox: &mut TextboxInfo<Color, Rectangle>,
        interface: &mut InterfaceState,
        rect: Rectangle,
    ) {
        let _ = textbox;
        interface.painter.draw_rectangle_rec(rect, Color::LIMEGREEN);
        interface.painter.gui_label(rect, "todo");
    }

    fn draw_image_editor(
        image: &mut ImageInfo<Vector2>,
        interface: &mut InterfaceState,
        rect: Rectangle,
    ) {
        let _ = image;
        interface.painter.draw_rectangle_rec(rect, Color::LIMEGREEN);
        interface.painter.gui_label(rect, "todo");
    }

    fn draw_pattern_editor(
        pattern: &mut PatternInfo<Color, Rectangle, Vector2>,
        interface: &mut InterfaceState,
        rect: Rectangle,
    ) {
        let _ = pattern;
        interface.painter.draw_rectangle_rec(rect, Color::LIMEGREEN);
        interface.painter.gui_label(rect, "todo");
    }

    fn draw_inspector(&mut self, interface: &mut InterfaceState, rect: Rectangle) {
        let title_rect = rect.resize_bottom(uiconsts::TITLE_HEIGHT);
        let content_rect = rect.expand_top(-uiconsts::TITLE_HEIGHT - uiconsts::CONTENT_GAP);
        let ui_rect =
            content_rect.resize_bottom(uiconsts::FONT_SIZE + 2.0 * uiconsts::BUTTON_PADDING);
        let list_rect = content_rect.expand_top(-ui_rect.height - uiconsts::CONTENT_GAP);

        interface.draw_title(title_rect, "Inspector");

        let [new_tb_rect, new_img_rect, new_pattern_rect, delete_rect] =
            ui_rect.divide_horizontal(uiconsts::CONTENT_GAP);

        interface.draw_text_button(new_tb_rect, "+ Textbox", true);
        interface.draw_text_button(new_img_rect, "+ Image", true);
        interface.draw_text_button(new_pattern_rect, "+ Pattern", true);
        interface.draw_text_button(delete_rect, "Delete", false);

        interface.painter.draw_rectangle_rec(list_rect, Color::PINK);
        interface.painter.gui_label(list_rect, "todo");
    }
}

/// raylib implementation of project editor
pub struct RaylibEditor {
    rl: RaylibHandle,

    thread: RaylibThread,

    state: RaylibEditorState,

    /// left mouse button state
    lmb_state: MouseButtonState,

    /// the font to use for titles
    title_font: Font,

    /// the font to use for general text
    font: Font,
}

impl RaylibEditor {
    pub fn new(project_directory: PathBuf, mut rl: RaylibHandle, thread: RaylibThread) -> Self {
        let title_font = rl
            .load_font_ex(
                &thread,
                uiconsts::DEFAULT_FONT_PATH,
                uiconsts::TITLE_FONT_SIZE as i32,
                None,
            )
            .expect("Could not load default font.");
        let font = rl
            .load_font_ex(
                &thread,
                uiconsts::DEFAULT_FONT_PATH,
                uiconsts::FONT_SIZE as i32,
                None,
            )
            .expect("Could not load default font.");

        Self {
            rl,
            thread,
            state: RaylibEditorState::new(project_directory),
            lmb_state: MouseButtonState::new(MouseButton::MOUSE_BUTTON_LEFT),
            font,
            title_font,
        }
    }

    pub fn run_standalone(project_directory: PathBuf) {
        let (rl, thread) = raylib::init()
            .vsync()
            .resizable()
            .title("Gatheround Game Editor")
            .build();

        let mut this = Self::new(project_directory, rl, thread);

        while this.update() {}
    }

    pub fn update(&mut self) -> bool {
        if self.rl.window_should_close() {
            return true;
        }

        self.lmb_state.update(&self.rl);

        let mut interface = InterfaceState {
            painter: self.rl.begin_drawing(&self.thread),
            font: &self.font,
            title_font: &self.title_font,
            lmb_state: &mut self.lmb_state,
        };
        self.state.update(&mut interface);


        true
    }
}
*/
