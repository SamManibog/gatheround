use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    path::Path,
    rc::Rc,
};

use mlua::MultiValue;
use raylib::{
    RaylibHandle, RaylibThread, callbacks,
    drawing::{
        RaylibBlendModeExt, RaylibDraw, RaylibDrawHandle, RaylibScissorModeExt,
        RaylibTextureModeExt,
    },
    ffi::{Color, KeyboardKey, Rectangle, TraceLogLevel, Vector2},
    text::{Font, RaylibFont},
    texture::{Image, RaylibTexture2D, RenderTexture2D},
};

use crate::{
    board::BoardManager,
    color::IColor,
    geometry::{ConvexHull, IConvexShape, IRect, IVec2},
    intermediate_representations::{
        Alignment, ImageTransform, PatternInfo, PatternType, TextboxInfo, VisualComponent,
        VisualInfo,
    },
    lua::get_game_lua_instance,
    renderer::{GlobalAssets, IRenderer, ITextureCompiler},
    resources::{PlayerColor, TextureInfo, VisualComponentProvider},
};

const FRAME_RATE: u32 = 60;

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

impl IConvexShape for Vector2 {
    fn furthest_point<V: IVec2>(&self, _direction: V) -> V {
        V::from_other(*self)
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

impl IConvexShape for Rectangle {
    fn furthest_point<V: IVec2>(&self, direction: V) -> V {
        IRect::furthest_point(self, direction)
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

pub struct RaylibTextureCompiler<'a> {
    pub rl: &'a mut RaylibHandle,
    pub thread: &'a RaylibThread,
}

impl<'a> RaylibTextureCompiler<'a> {
    /// padding for compiled textures
    const TEXTURE_PADDING: f32 = 2.0;

    /// vertical padding for fonts
    const TEXTBOX_V_PADDING: f32 = 2.0;

    /// horizontal padding for fonts
    const TEXTBOX_H_PADDING: f32 = 2.0;

    /// minimum font size for automatic font sizing
    const MIN_AUTO_FONT_SIZE: f32 = 4.0;

    /// minimum line spacing for automatic line spacing
    const MIN_AUTO_LINE_SPACING: f32 = 1.05;

    fn get_fitting_text_size(font: &Font, text: &str, max_line_length: f32) -> f32 {
        let base_size = 32.0;
        let actual_size = font.measure_text(text, base_size, 0.0).x;

        base_size * max_line_length / actual_size
    }

    fn draw_texture(
        &mut self,
        transform: &ImageTransform<Vector2>,
        texture: &RenderTexture2D,
        target: &mut RenderTexture2D,
        tint: Color,
    ) {
        self.rl
            .draw_texture_mode(&self.thread, target, |mut painter| {
                let src_rect = Rectangle::from_position_dimension(
                    (0.0, 0.0),
                    (texture.width() as f32, texture.height() as f32).neg_y(),
                );
                let dst_rect = Rectangle::from_center_dimension(
                    transform.position,
                    src_rect
                        .dimensions::<Vector2>()
                        .scaled(transform.scale)
                        .neg_y(),
                );
                painter.draw_texture_pro(texture, src_rect, dst_rect, Vector2::ZERO, 0.0, tint);
            });
    }

    fn draw_textbox(
        &mut self,
        textbox: &TextboxInfo<Color, Rectangle>,
        visual_component_provider: &mut VisualComponentProvider<'_, Font, RenderTexture2D>,
        transform: &ImageTransform<Vector2>,
        target: &mut RenderTexture2D,
    ) {
        // attempt to get the font, return if font could not be found
        let font = if let Some(f) = visual_component_provider.get_or_load_font(
            self,
            &textbox.font,
            textbox.size.unwrap_or(32),
            &textbox.text,
        ) {
            f
        } else {
            return;
        };

        // the true draw rectangle
        let rect: Rectangle = transform
            .transform_rect(textbox.rect)
            .expand_vertical(-Self::TEXTBOX_V_PADDING)
            .expand_horizontal(-Self::TEXTBOX_H_PADDING);

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
        let (font_size, line_increment) = match (textbox.size, textbox.spacing) {
            (None, None) => {
                let width_sized = Self::get_fitting_text_size(&font, &textbox.text, rect.width());

                if lines.len() == 1 {
                    let size = width_sized.min(rect.height()).max(Self::MIN_AUTO_FONT_SIZE);
                    (size.floor(), 0.0)
                } else {
                    let default_spacing = Self::MIN_AUTO_LINE_SPACING.max(1.1);
                    let height_sized =
                        rect.height() / (1.0 + default_spacing * (lines.len() as f32 - 1.0));
                    let size = width_sized.min(height_sized).max(Self::MIN_AUTO_FONT_SIZE);

                    (size.floor(), (default_spacing * size).floor())
                }
            }

            (Some(size), None) => {
                let size = size as f32 * transform.scale;

                if lines.len() == 1 {
                    (size.floor(), 0.0)
                } else {
                    let remaining_space = rect.height - size * lines.len() as f32;
                    let spacing = (remaining_space / (lines.len() as f32 - 1.0))
                        .max(size * Self::MIN_AUTO_LINE_SPACING);
                    (size.floor(), (size + spacing).floor())
                }
            }

            (None, Some(spacing)) => {
                // determine maximum size based on textbox width
                let width_sized = Self::get_fitting_text_size(&font, &textbox.text, rect.width());

                // determine maximum sized based on textbox height
                // let height_sized = rect.height() / (lines.len() as f32 * (1.0 + spacing) - spacing);
                let height_sized = rect.height() / (1.0 + spacing * (lines.len() as f32 - 1.0));

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
            &textbox.font,
            font_size as u32,
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
        let mut painter = self.rl.begin_texture_mode(&self.thread, target);
        painter.draw_blend_mode(
            raylib::ffi::BlendMode::BLEND_ALPHA_PREMULTIPLY,
            |mut painter| {
                let color = textbox.color;
                match textbox.align.horizontal_align() {
                    Alignment::Begin => {
                        for line in lines {
                            let position = Vector2::from_xy(rect.x, y_cursor);
                            painter.draw_text_ex(&font, line, position, font_size, 0.0, color);

                            y_cursor += line_increment;
                        }
                    }

                    Alignment::Center => {
                        for line in lines {
                            let line_length = font.measure_text(&line, font_size, 0.0).x;

                            let position = Vector2::from_xy(
                                (rect.width - line_length) / 2.0 + rect.x,
                                y_cursor,
                            );
                            painter.draw_text_ex(&font, line, position, font_size, 0.0, color);

                            y_cursor += line_increment;
                        }
                    }

                    Alignment::End => {
                        for line in lines {
                            let line_length = font.measure_text(&line, font_size, 0.0).x;

                            let position =
                                Vector2::from_xy(rect.width - line_length + rect.x, y_cursor);
                            painter.draw_text_ex(&font, line, position, font_size, 0.0, color);

                            y_cursor += line_increment;
                        }
                    }
                }
            },
        );
    }

    fn draw_pattern(
        &mut self,
        pattern: &PatternInfo<Color, Rectangle, Vector2>,
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

        let src_rect = Rectangle::from_center_dimension(
            pattern_dimensions.scaled(0.5).add(pattern.true_offset()),
            pattern_dimensions.neg_y(),
        );
        let dst_rect =
            Rectangle::from_position_dimension(rect.center::<Vector2>(), pattern_dimensions);

        painter.draw_scissor_mode(
            rect.x as i32,
            rect.y as i32,
            rect.width as i32,
            rect.height as i32,
            |mut painter| {
                painter.draw_texture_pro(
                    &pattern_texture,
                    src_rect,
                    dst_rect,
                    dst_rect.dimensions::<Vector2>().scaled(0.5),
                    pattern.angle,
                    Color::WHITE,
                );
            },
        );
    }

    fn draw_solid_pattern(
        &mut self,
        pattern: &PatternInfo<Color, Rectangle, Vector2>,
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

    fn make_checkers(
        &mut self,
        pattern: &PatternInfo<Color, Rectangle, Vector2>,
        transform: &ImageTransform<Vector2>,
    ) -> Option<RenderTexture2D> {
        let rect: Rectangle = transform.transform_rect(pattern.rect);

        // determine actual values
        let weight = pattern.weight * transform.scale;
        let spacing = pattern.spacing * transform.scale;
        let increment = weight + spacing;

        // create the pattern texture
        let pattern_side_length = rect
            .dimensions::<Vector2>()
            .add(pattern.true_offset().abs())
            .length()
            .ceil() as u32
            + 2;
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
        painter.clear_background(background);

        // make the foreground
        let color = Color::from_other(&pattern.pattern_color);

        // draw individual checkers
        let square_count = {
            let maybe_odd = (pattern_side_length as f32 / increment + 1.0).ceil() as u32;
            if maybe_odd % 2 == 1 {
                maybe_odd + 1
            } else {
                maybe_odd
            }
        };
        let checkers_length = square_count as f32 * increment;
        let start_coord = pattern_side_length as f32 / 2.0 - checkers_length / 2.0;
        let mut x_cursor = start_coord;
        for x in 0..square_count {
            let mut y_cursor = start_coord;

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

    fn make_stripes(
        &mut self,
        pattern: &PatternInfo<Color, Rectangle, Vector2>,
        transform: &ImageTransform<Vector2>,
    ) -> Option<RenderTexture2D> {
        let rect: Rectangle = transform.transform_rect(pattern.rect);

        // determine actual values
        let weight = pattern.weight * transform.scale;
        let spacing = pattern.spacing * transform.scale;
        let increment = weight + spacing;

        // create the pattern texture
        let pattern_side_length = rect
            .dimensions::<Vector2>()
            .add(pattern.true_offset().abs())
            .length()
            .ceil() as u32
            + 2;
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
        painter.clear_background(background);

        // make the foreground
        let color = Color::from_other(&pattern.pattern_color);

        // draw individual checkers
        let stripe_count = (pattern_side_length as f32 / increment).ceil() as u32 * 2;
        let stripes_length = (stripe_count / 2) as f32 * increment;
        let mut cursor = pattern_side_length as f32 / 2.0 - stripes_length / 2.0;
        for x in 0..stripe_count {
            if x % 2 == 0 {
                let r = Rectangle {
                    x: cursor,
                    y: 0.0,
                    width: weight,
                    height,
                };

                painter.draw_rectangle_rec(r, color);
                cursor += weight;
            } else {
                cursor += spacing;
            }
        }

        drop(painter);

        Some(pattern_texture)
    }

    fn make_dots(
        &mut self,
        pattern: &PatternInfo<Color, Rectangle, Vector2>,
        transform: &ImageTransform<Vector2>,
    ) -> Option<RenderTexture2D> {
        let rect: Rectangle = transform.transform_rect(pattern.rect);

        // determine actual values
        let weight = pattern.weight * transform.scale;
        let spacing = pattern.spacing * transform.scale;
        let increment = weight + spacing;

        // create the pattern texture
        let pattern_side_length = rect
            .dimensions::<Vector2>()
            .add(pattern.true_offset().abs())
            .length()
            .ceil() as u32
            + 2;
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
        painter.clear_background(background);

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

impl<'a> ITextureCompiler for RaylibTextureCompiler<'a> {
    type Texture = RenderTexture2D;
    type Font = Font;
    type Color = Color;
    type Rectangle = Rectangle;
    type Vector2 = Vector2;

    fn compile_visual(
        &mut self,
        visual: &VisualInfo<Color, Rectangle, Vector2>,
        magnification_dimensions: Vector2,
        mut visual_component_provider: VisualComponentProvider<'_, Self::Font, Self::Texture>,
    ) -> Result<TextureInfo<Self::Texture>, String> {
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
                            let r = Rectangle::from_center_dimension(
                                i.transform.position,
                                Vector2::from_xy(
                                    image_texture.width() as f32,
                                    image_texture.height() as f32,
                                )
                                .scaled(i.transform.scale),
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
            position: Vector2::from_xy(width as f32, height as f32).scale(0.5),
            scale: 1.0,
        };

        // create the magnified texture
        let magnification = {
            let x_scale = magnification_dimensions.x() / render_dimensions.x.max(1.0);
            let y_scale = magnification_dimensions.y() / render_dimensions.y.max(1.0);

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
            position: Vector2::from_xy(mangnified_width as f32, magnified_height as f32).scale(0.5),
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
                            i.tint,
                        );
                        self.draw_texture(
                            &magnified_transform.transform_other(&i.transform),
                            &image,
                            &mut texture_magnified,
                            i.tint,
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
            texture,
            texture_magnified,
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

    fn load_texture(&mut self, path: &std::path::Path) -> Result<RenderTexture2D, String> {
        match self.rl.load_texture(&self.thread, &path.to_string_lossy()) {
            Ok(nonrender_texture) => {
                let mut texture = match self.rl.load_render_texture(
                    self.thread,
                    nonrender_texture.width as u32,
                    nonrender_texture.height as u32,
                ) {
                    Ok(t) => t,
                    Err(msg) => {
                        return Err(format!(
                            "Could not load texture from '{}': {msg}",
                            path.to_string_lossy()
                        ));
                    }
                };
                self.rl
                    .draw_texture_mode(self.thread, &mut texture, |mut painter| {
                        painter.draw_texture(&nonrender_texture, 0, 0, Color::WHITE);
                    });
                Ok(texture)
            }

            Err(err) => Err(format!(
                "Could not load texture from '{}': {err}",
                path.to_string_lossy()
            )),
        }
    }

    fn load_texture_from_memory(
        &mut self,
        filetype: &str,
        bytes: &[u8],
    ) -> Result<Self::Texture, String> {
        let image = match Image::load_image_from_mem(filetype, bytes) {
            Ok(i) => i,
            Err(msg) => return Err(format!("Failed to load texture from memory: {msg}")),
        };
        match self.rl.load_texture_from_image(&self.thread, &image) {
            Ok(nonrender_texture) => {
                let mut texture = match self.rl.load_render_texture(
                    self.thread,
                    nonrender_texture.width as u32,
                    nonrender_texture.height as u32,
                ) {
                    Ok(t) => t,
                    Err(msg) => return Err(format!("Failed to load texture from memory: {msg}")),
                };
                self.rl
                    .draw_texture_mode(self.thread, &mut texture, |mut painter| {
                        painter.draw_texture(&nonrender_texture, 0, 0, Color::WHITE);
                    });
                Ok(texture)
            }

            Err(msg) => Err(format!("Failed to load texture from memory: {msg}")),
        }
    }
}

pub struct RaylibRenderer<'a> {
    pub rl: &'a mut RaylibHandle,
    pub thread: &'a RaylibThread,
    pub draw_layer_pool: &'a mut Vec<RenderTexture2D>,
}

impl<'a> RaylibRenderer<'a> {
    /// thickness for debug polygon lines
    const DEBUG_THICKNESS: f32 = 1.0;

    /// thickness for debug polygon line outlines
    const DEBUG_OUTLINE_THICKNESS: f32 = 3.0;
}

impl<'a> IRenderer for RaylibRenderer<'a> {
    type Texture = RenderTexture2D;
    type Font = Font;
    type Color = Color;
    type Rectangle = Rectangle;
    type Vector2 = Vector2;
    type ConvexShape = ConvexHull<Vector2>;

    fn draw_board(
        &mut self,
        board: &crate::board::BoardManager<
            Color,
            Rectangle,
            Vector2,
            ConvexHull<Vector2>,
            Self::Texture,
            Self::Font,
        >,
        assets: &GlobalAssets<Self::Texture>,
        output_texture: &mut Self::Texture,
    ) {
        let output_dimensions = board.dimensions();
        for _ in 0..board
            .draw_layer_count()
            .saturating_sub(self.draw_layer_pool.len())
        {
            self.draw_layer_pool.push(
                self.rl
                    .load_render_texture(
                        self.thread,
                        output_dimensions.x() as u32,
                        output_dimensions.y() as u32,
                    )
                    .unwrap(),
            );
        }

        let mut pool_iter = self.draw_layer_pool.iter_mut();
        let mut draw_layers = BTreeMap::from_iter(board.draw_layer_iter().map(|l| {
            let mut layer = pool_iter.next().unwrap();
            self.rl
                .draw_texture_mode(self.thread, &mut layer, |mut painter| {
                    painter.clear_background(Color::BLANK)
                });
            (l, layer)
        }));

        // collision layers for debug collision layer drawing
        let mut collision_layers = BTreeMap::new();
        let mut collision_layer_number = 0.0;

        for (texture, collider_opt, transform, layer) in board.texture_draw_iter() {
            let render_texture = draw_layers.get_mut(&layer).unwrap();
            let mut painter = self.rl.begin_texture_mode(self.thread, render_texture);

            // draw texture
            let src_rect = Rectangle {
                x: 0.0,
                y: 0.0,
                width: texture.width() as f32,
                height: -texture.height() as f32,
            };
            let dst_rect = Rectangle::from_position_dimension(
                transform.position,
                src_rect.dimensions::<Vector2>().neg_y(),
            );
            painter.draw_texture_pro(
                &texture,
                src_rect,
                dst_rect,
                dst_rect.dimensions::<Vector2>().scaled(0.5),
                transform.angle.to_degrees(),
                Color::WHITE,
            );

            // draw colliders
            if let Some(colliders) = collider_opt {
                for collider in colliders {
                    let color = if let Some(c) = collision_layers.get(&collider.layer) {
                        *c
                    } else {
                        let hue = collision_layer_number / board.collision_layer_count() as f32;
                        collision_layer_number += 1.0;
                        let color = Color::color_from_hsv(hue, 1.0, 1.0);
                        collision_layers.insert(collider.layer, color);
                        color
                    };

                    let shapes = board.get_collision_shape(collider.shape).unwrap();
                    for shape in shapes.0.iter() {
                        let points = shape.points();
                        let mut point_iter = shape.points().iter().peekable();
                        while let (Some(p1), Some(p2)) = (point_iter.next(), point_iter.peek()) {
                            let p1 = transform.apply_to_point(*p1);
                            let p2 = transform.apply_to_point(**p2);
                            painter.draw_line_ex(
                                p1,
                                p2,
                                Self::DEBUG_OUTLINE_THICKNESS,
                                Color::BLACK,
                            );
                            painter.draw_line_ex(p1, p2, Self::DEBUG_THICKNESS, color);
                        }
                        let p1 = transform.apply_to_point(*points.first().unwrap());
                        let p2 = transform.apply_to_point(*points.last().unwrap());
                        painter.draw_line_ex(p1, p2, Self::DEBUG_OUTLINE_THICKNESS, Color::BLACK);
                        painter.draw_line_ex(p1, p2, Self::DEBUG_THICKNESS, color);
                    }
                }
            }
        }

        self.rl
            .draw_texture_mode(self.thread, output_texture, |mut painter| {
                painter.clear_background(board.color);

                // draw regular layers
                for layer in draw_layers.values() {
                    let src_rect = Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: layer.width() as f32,
                        height: -layer.height() as f32,
                    };
                    painter.draw_texture_rec(
                        &layer,
                        src_rect,
                        Vector2::from_xy(0.0, 0.0),
                        Color::WHITE,
                    );
                }

                // draw grabbed elements (again)
                for (texture, collider_opt, transform) in board.held_item_draw_iter() {
                    let src_rect = Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: texture.width() as f32,
                        height: -texture.height() as f32,
                    };
                    let dst_rect = Rectangle::from_position_dimension(
                        transform.position,
                        src_rect.dimensions::<Vector2>().neg_y(),
                    );
                    painter.draw_texture_pro(
                        &texture,
                        src_rect,
                        dst_rect,
                        dst_rect.dimensions::<Vector2>().scaled(0.5),
                        transform.angle.to_degrees(),
                        Color::WHITE,
                    );

                    // draw colliders
                    if let Some(colliders) = collider_opt {
                        for collider in colliders {
                            let color = if let Some(c) = collision_layers.get(&collider.layer) {
                                *c
                            } else {
                                let hue =
                                    collision_layer_number / board.collision_layer_count() as f32;
                                collision_layer_number += 1.0;
                                let color = Color::color_from_hsv(hue, 1.0, 1.0);
                                collision_layers.insert(collider.layer, color);
                                color
                            };

                            let shapes = board.get_collision_shape(collider.shape).unwrap();
                            for shape in shapes.0.iter() {
                                let points = shape.points();
                                let mut point_iter = shape.points().iter().peekable();
                                while let (Some(p1), Some(p2)) =
                                    (point_iter.next(), point_iter.peek())
                                {
                                    let p1 = transform.apply_to_point(*p1);
                                    let p2 = transform.apply_to_point(**p2);
                                    painter.draw_line_ex(
                                        p1,
                                        p2,
                                        Self::DEBUG_OUTLINE_THICKNESS,
                                        Color::BLACK,
                                    );
                                    painter.draw_line_ex(p1, p2, Self::DEBUG_THICKNESS, color);
                                }
                                let p1 = transform.apply_to_point(*points.first().unwrap());
                                let p2 = transform.apply_to_point(*points.last().unwrap());
                                painter.draw_line_ex(
                                    p1,
                                    p2,
                                    Self::DEBUG_OUTLINE_THICKNESS,
                                    Color::BLACK,
                                );
                                painter.draw_line_ex(p1, p2, Self::DEBUG_THICKNESS, color);
                            }
                        }
                    }
                }

                // draw player hands
                for (texture, position, tint) in board.player_draw_iter(&assets) {
                    let src_rect = Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: texture.width() as f32,
                        height: -texture.height() as f32,
                    };
                    let dst_rect = Rectangle::from_position_dimension(
                        position,
                        src_rect.dimensions::<Vector2>().neg_y(),
                    );
                    painter.draw_texture_pro(
                        &texture,
                        src_rect,
                        dst_rect,
                        dst_rect.dimensions::<Vector2>().scaled(0.5),
                        0.0,
                        tint,
                    );
                }

                // draw snap circles
                for (position, radius) in board.debug_snap_circle_draw_iter() {
                    let segments = (radius.ceil() as i32 * 2).min(24);
                    painter.draw_ring(
                        position,
                        (radius - Self::DEBUG_OUTLINE_THICKNESS * 0.5).max(0.0),
                        radius + Self::DEBUG_OUTLINE_THICKNESS * 0.5,
                        0.0,
                        360.0,
                        segments,
                        Color::BLACK,
                    );
                    painter.draw_ring(
                        position,
                        (radius - Self::DEBUG_THICKNESS * 0.5).max(0.0),
                        radius + Self::DEBUG_THICKNESS * 0.5,
                        0.0,
                        360.0,
                        segments,
                        Color::WHITE,
                    );
                }
            });
    }
}

/// a simple text buffer for running lua commands
#[derive(Debug)]
pub struct TextCommandBuffer {
    /// command history
    history: VecDeque<String>,

    /// index in command history
    history_index: usize,

    /// command being entered now
    command: String,

    /// byte index in the command
    /// invariant: index always lies on a valid utf8 char boundary
    index: usize,

    /// the font used for this thinggy
    font: Font,

    /// the current frame used to calculate cursor blinking effects
    blink_frame: u32,

    /// frame-based cooldown for holding up key
    up_cooldown: u32,

    /// frame-based cooldown for holding down key
    down_cooldown: u32,

    /// frame-based cooldown for holding left key
    left_cooldown: u32,

    /// frame-based cooldown for holding right key
    right_cooldown: u32,

    /// frame-based cooldown for holding delete key
    del_cooldown: u32,

    /// frame-based cooldown for holding backspace key
    bs_cooldown: u32,
}

impl TextCommandBuffer {
    /// the default font, should be monospace
    pub const DEFAULT_FONT: &str = r"C:\Windows\Fonts/lucon.ttf";
    pub const MAX_HISTORY: usize = 250;

    const BLINK_FRAMES: u32 = FRAME_RATE / 2;
    const SPAM_COOLDOWN: u32 = FRAME_RATE / 12;

    const TEXT_SIZE: f32 = 28.0;
    const SPACING: f32 = 1.0;
    const PADDING: f32 = 4.0;
    const BACKGROUND_COLOR: Color = Color::BLACK;
    const FOREGROUND_COLOR: Color = Color {
        r: 0,
        g: 240,
        b: 0,
        a: 255,
    };

    pub fn new(rl: &mut RaylibHandle, thread: &RaylibThread) -> Self {
        let font = rl
            .load_font_ex(thread, Self::DEFAULT_FONT, Self::TEXT_SIZE as i32, None)
            .expect(&format!(
                "Unable to load command buffer font '{}'",
                Self::DEFAULT_FONT
            ));

        Self {
            history: VecDeque::with_capacity(Self::MAX_HISTORY),
            history_index: 0,
            command: String::new(),
            index: 0,
            font,
            blink_frame: 0,
            up_cooldown: 0,
            down_cooldown: 0,
            left_cooldown: 0,
            right_cooldown: 0,
            del_cooldown: 0,
            bs_cooldown: 0,
        }
    }

    /// decrements all cooldowns
    fn decrement_cooldowns(&mut self) {
        self.up_cooldown = self.up_cooldown.saturating_sub(1);
        self.down_cooldown = self.down_cooldown.saturating_sub(1);
        self.left_cooldown = self.left_cooldown.saturating_sub(1);
        self.right_cooldown = self.right_cooldown.saturating_sub(1);
        self.del_cooldown = self.del_cooldown.saturating_sub(1);
        self.bs_cooldown = self.bs_cooldown.saturating_sub(1);
    }

    /// increments the cursor
    fn increment_cursor(&mut self) {
        if let Some(ch) = self.command[self.index..].chars().next() {
            self.index += ch.len_utf8();
        }
    }

    /// decrements the cursor
    fn decrement_cursor(&mut self) {
        self.index = self.index.saturating_sub(1);

        while self.index > 0 && !self.command.is_char_boundary(self.index) {
            self.index -= 1;
        }
    }

    /// gets the currently entered command
    pub fn command(&self) -> &str {
        &self.command
    }

    /// gets the character at the cursor if it exists
    fn cursor_str(&self) -> Option<&str> {
        if self.index == self.command.len() {
            return None;
        }

        let mut chars = self.command[self.index..].chars();

        chars.next().map(|c| {
            let end_idx = self.index + c.len_utf8();
            &self.command[self.index..end_idx]
        })
    }

    /// updates the buffer with keyboard inputs
    pub fn update(&mut self, rl: &mut RaylibHandle) -> Option<String> {
        self.blink_frame = self.blink_frame.wrapping_add(1);
        self.decrement_cooldowns();

        // history traversal
        if rl.is_key_down(KeyboardKey::KEY_UP) {
            if self.up_cooldown <= 0 {
                self.blink_frame = 0;
                self.up_cooldown = Self::SPAM_COOLDOWN;

                self.history_index = self.history_index.saturating_sub(1);
                if let Some(hist_command) = self.history.get(self.history_index) {
                    self.command = hist_command.clone();
                } else {
                    self.command = String::new();
                }
                self.index = self.command.len();
            }
        } else {
            self.up_cooldown = 0;
        }
        if rl.is_key_down(KeyboardKey::KEY_DOWN) {
            if self.down_cooldown <= 0 {
                self.blink_frame = 0;
                self.down_cooldown = Self::SPAM_COOLDOWN;

                if self.history_index < self.history.len().saturating_sub(1) {
                    self.history_index += 1;
                    self.command = self.history[self.history_index].clone();
                }
                self.index = self.command.len();
            }
        } else {
            self.down_cooldown = 0;
        }

        // handle normal character entry
        while let Some(ch) = rl.get_char_pressed() {
            self.blink_frame = 0;

            self.command.insert(self.index, ch);
            self.index += 1;
        }

        // handle movement
        if rl.is_key_down(KeyboardKey::KEY_LEFT) {
            if self.left_cooldown <= 0 {
                self.blink_frame = 0;
                self.left_cooldown = Self::SPAM_COOLDOWN;

                self.decrement_cursor();
            }
        } else {
            self.left_cooldown = 0;
        }
        if rl.is_key_down(KeyboardKey::KEY_RIGHT) {
            if self.right_cooldown <= 0 {
                self.blink_frame = 0;
                self.right_cooldown = Self::SPAM_COOLDOWN;

                self.increment_cursor();
            }
        } else {
            self.right_cooldown = 0;
        }

        // handle deletion
        if rl.is_key_down(KeyboardKey::KEY_BACKSPACE) {
            if self.bs_cooldown <= 0 {
                self.blink_frame = 0;
                self.bs_cooldown = Self::SPAM_COOLDOWN;

                self.decrement_cursor();
                if let Some((idx, _)) = self.command.char_indices().nth(self.index) {
                    self.command.remove(idx);
                }
            }
        } else {
            self.bs_cooldown = 0;
        }
        if rl.is_key_down(KeyboardKey::KEY_DELETE) {
            if self.del_cooldown <= 0 {
                self.blink_frame = 0;
                self.del_cooldown = Self::SPAM_COOLDOWN;

                if let Some((idx, _)) = self.command.char_indices().nth(self.index) {
                    self.command.remove(idx);
                }
            }
        } else {
            self.del_cooldown = 0;
        }

        // handle the enter key
        if rl.is_key_pressed(KeyboardKey::KEY_ENTER) {
            self.blink_frame = 0;
            // adjust history
            if self.history.len() >= Self::MAX_HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(self.command.clone());
            self.history_index = self.history.len();

            self.index = 0;
            let mut output = String::new();
            std::mem::swap(&mut output, &mut self.command);
            Some(output)
        } else {
            None
        }
    }

    /// draws to the screen at the given position
    pub fn draw(&self, position: Vector2, painter: &mut RaylibDrawHandle) {
        if self.command.is_empty() {
            return;
        }

        let text_dimensions =
            self.font
                .measure_text(&self.command, Self::TEXT_SIZE as f32, Self::SPACING);
        let cursor_position = self
            .font
            .measure_text(
                &self.command[0..self.index],
                Self::TEXT_SIZE as f32,
                Self::SPACING,
            )
            .with_y(0.0)
            .add(position);
        let cursor_dimensions = self
            .font
            .measure_text(&"$", Self::TEXT_SIZE as f32, Self::SPACING);

        let cursor_rect = Rectangle::from_position_dimension(cursor_position, cursor_dimensions);
        let big_cursor_rect = cursor_rect.expand_vertical(Self::PADDING);
        let background_rect = Rectangle::from_position_dimension(position, text_dimensions)
            .expand(Self::PADDING)
            .expand_right(big_cursor_rect.width);

        let should_draw_cursor = self.blink_frame % (Self::BLINK_FRAMES * 2) < Self::BLINK_FRAMES;

        painter.draw_rectangle_rec(background_rect, Self::BACKGROUND_COLOR);
        if should_draw_cursor {
            painter.draw_rectangle_rec(big_cursor_rect, Self::FOREGROUND_COLOR);
        }

        painter.draw_text_ex(
            &self.font,
            &self.command,
            position,
            Self::TEXT_SIZE,
            Self::SPACING,
            Self::FOREGROUND_COLOR,
        );

        if should_draw_cursor {
            painter.draw_rectangle_rec(cursor_rect, Self::FOREGROUND_COLOR);
            if let Some(cursor_str) = self.cursor_str() {
                painter.draw_text_ex(
                    &self.font,
                    cursor_str,
                    cursor_rect.position::<Vector2>(),
                    Self::TEXT_SIZE,
                    Self::SPACING,
                    Self::BACKGROUND_COLOR,
                );
            }
        }
    }
}

/// runs a game editor from the given project path
pub fn run_game(project_path: &Path) {
    let test_player = PlayerColor::Lime;

    // initialize game resources
    let board_dimensions = Vector2::from_xy(1920.0, 1080.0);
    let mut board =
        BoardManager::<Color, Rectangle, Vector2, ConvexHull<Vector2>, RenderTexture2D, Font>::new(
            board_dimensions,
        );
    let _ = board.add_player(test_player, "Player".to_string());
    let board_cell = Rc::new(RefCell::new(board));

    // initialize raylib
    callbacks::set_trace_log_callback(|level, msg| match level {
        TraceLogLevel::LOG_WARNING => println!("{msg}"),
        TraceLogLevel::LOG_ERROR => eprintln!("{msg}"),
        TraceLogLevel::LOG_FATAL => eprintln!("{msg}"),
        _ => (),
    })
    .expect("unable to set trace log level");
    let mut draw_layer_pool = Vec::new();
    let (mut rl, thread) = raylib::init()
        .vsync()
        .title("Gatheround Board Game Engine")
        //.fullscreen()
        .size(board_dimensions.x() as i32, board_dimensions.y() as i32)
        .resizable()
        .build();
    rl.set_exit_key(None);
    rl.set_target_fps(FRAME_RATE);
    let mut output_texture = rl
        .load_render_texture(
            &thread,
            board_cell.borrow().dimensions().x() as u32,
            board_cell.borrow().dimensions().y() as u32,
        )
        .expect("Unable to create board render layer");
    let global_assets = GlobalAssets::new(&mut RaylibTextureCompiler {
        rl: &mut rl,
        thread: &thread,
    });

    let mut command_buffer = TextCommandBuffer::new(&mut rl, &thread);
    let rl_cell = Rc::new(RefCell::new(rl));
    let thread_cell = Rc::new(RefCell::new(thread));

    // get and run init function
    let lua = get_game_lua_instance(&project_path, &board_cell, &rl_cell, &thread_cell);

    // run main loop
    loop {
        // detect window close
        if rl_cell.borrow_mut().window_should_close() {
            return;
        }

        let padding = {
            let rl = rl_cell.borrow();
            let board = board_cell.borrow();
            Vector2::from_xy(
                rl.get_render_width() as f32 - board.dimensions().x(),
                rl.get_render_height() as f32 - board.dimensions().y(),
            )
            .scaled(0.5)
        };

        // update hand position
        // randomize hand update order for "fair" movement
        {
            let rl = rl_cell.borrow_mut();
            if rl.is_mouse_button_pressed(raylib::ffi::MouseButton::MOUSE_BUTTON_LEFT) {
                drop(rl);

                let grab_attempted_fn: mlua::Function = lua
                    .globals()
                    .raw_get("grabAttempted")
                    .expect("grabAttempted has been overwritten to a non-function value.");
                let name_string = lua
                    .create_string(test_player.name())
                    .expect("unable to create player name string");

                let _: MultiValue = grab_attempted_fn
                    .call(name_string)
                    .expect("grabAttempted has been overwritten with an invalid function.");
            } else if rl.is_mouse_button_pressed(raylib::ffi::MouseButton::MOUSE_BUTTON_RIGHT) {
                drop(rl);

                let drop_attempted_fn: mlua::Function = lua
                    .globals()
                    .raw_get("dropAttempted")
                    .expect("dropAttempted has been overwritten to a non-function value.");
                let name_string = lua
                    .create_string(test_player.name())
                    .expect("unable to create player name string");

                let _: MultiValue = drop_attempted_fn
                    .call(name_string)
                    .expect("dropAttempted has been overwritten with an invalid function.");
            }
        }

        // update positions
        {
            let rl = rl_cell.borrow_mut();
            let mut board = board_cell.borrow_mut();
            board
                .set_player_position(test_player, rl.get_mouse_position().sub(padding))
                .expect("player should be valid");
        }
        {
            let player_moved_fn: mlua::Function = lua
                .globals()
                .raw_get("playerMoved")
                .expect("playerMoved has been overwritten to a non-function value.");
            let name_string = lua
                .create_string(test_player.name())
                .expect("unable to create player name string");

            let _: MultiValue = player_moved_fn
                .call(name_string)
                .expect("playerMoved has been overwritten with an invalid function.");
        }

        // update commands
        {
            let mut rl = rl_cell.borrow_mut();
            if let Some(cmd) = command_buffer.update(&mut rl) {
                drop(rl);

                match lua.load(cmd).eval::<mlua::MultiValue>() {
                    Ok(v) => println!("GOTVALUE FROM COMMAND!: {:?}", v),
                    Err(e) => {
                        println!("GOT ERROR FROM COMMAND! NOT YET SURE WHAT TO DO WITH IT:\n{e}")
                    }
                }
            }
        }

        // run draw cycle
        {
            let mut rl = rl_cell.borrow_mut();
            let thread = thread_cell.borrow();
            let mut rl_renderer = RaylibRenderer {
                rl: &mut rl,
                thread: &thread,
                draw_layer_pool: &mut draw_layer_pool,
            };
            rl_renderer.draw_board(&board_cell.borrow(), &global_assets, &mut output_texture);

            rl.draw(&thread, |mut painter| {
                painter.clear_background(board_cell.borrow().color);

                let src_rect =
                    Rectangle::from_position_dimension((0.0, 0.0), board_dimensions.neg_y());
                let dst_rect = Rectangle::from_position_dimension(padding, board_dimensions);
                painter.draw_texture_pro(
                    &output_texture,
                    src_rect,
                    dst_rect,
                    Vector2::from_xy(0.0, 0.0),
                    0.0,
                    Color::WHITE,
                );

                command_buffer.draw(Vector2::from_xy(40.0, 40.0), &mut painter);
            });
        }
    }
}
