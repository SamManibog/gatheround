use std::collections::BTreeMap;

use raylib::{
    RaylibHandle, RaylibThread, callbacks,
    drawing::{RaylibBlendModeExt, RaylibDraw, RaylibScissorModeExt, RaylibTextureModeExt},
    ffi::{Color, KeyboardKey, Rectangle, TraceLogLevel, Vector2},
    text::{Font, RaylibFont},
    texture::{Image, RaylibTexture2D, RenderTexture2D},
};

use crate::{
    board::BoardManager,
    cheat_buffer::CheatBuffer,
    color::IColor,
    geometry::{ConvexHull, IConvexShape, IRect, IVec2},
    intermediate_representations::{
        Alignment, ImagePosition, ImageTransform, PatternInfo, PatternType, TextboxInfo,
        VisualComponent, VisualInfo,
    },
    platform::{GlobalAssets, IKeyboardReader, IPlatform, IRenderer, ITextureCompiler},
    resources::{TextureInfo, VisualComponentProvider},
};

const TARGET_FRAME_RATE: u32 = 60;

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

#[derive(Debug)]
pub struct RaylibTextureCompiler<'a> {
    rl: &'a mut RaylibHandle,
    thread: &'a RaylibThread,
}

impl<'a> RaylibTextureCompiler<'a> {
    /// padding for compiled textures
    const TEXTURE_PADDING: u32 = 2;

    /// vertical padding for fonts
    /// as a proportion of textbox size
    const TEXTBOX_V_PADDING: f32 = 0.0;

    /// horizontal padding for fonts
    /// as a proportion of textbox size
    const TEXTBOX_H_PADDING: f32 = 0.02;

    /// minimum font size for automatic font sizing
    const MIN_AUTO_FONT_SIZE: f32 = 4.0;

    /// minimum line spacing for automatic line spacing
    const MIN_AUTO_LINE_SPACING: f32 = 1.05;

    fn get_fitting_text_size(font: &Font, text: &str, max_line_length: f32) -> f32 {
        let base_size = 64.0;
        let actual_size = (font.measure_text(text, base_size, 0.0).x).ceil();

        (base_size * max_line_length / actual_size).floor()
    }

    fn draw_texture(
        &mut self,
        position: ImagePosition<Rectangle, Vector2>,
        texture: &RenderTexture2D,
        target: &mut RenderTexture2D,
        tint: Color,
    ) {
        match position {
            ImagePosition::Transform(transform) => {
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
                        painter.draw_texture_pro(
                            texture,
                            src_rect,
                            dst_rect,
                            Vector2::ZERO,
                            0.0,
                            tint,
                        );
                    });
            }

            ImagePosition::FitRect(rect) => {
                self.rl
                    .draw_texture_mode(&self.thread, target, |mut painter| {
                        let src_rect = Rectangle::from_position_dimension(
                            (0.0, 0.0),
                            (texture.width() as f32, texture.height() as f32).neg_y(),
                        );
                        let dst_rect = rect.centered_with_aspect_ratio((
                            texture.width() as f32,
                            texture.height() as f32,
                        ));
                        painter.draw_texture_pro(
                            texture,
                            src_rect,
                            dst_rect,
                            Vector2::ZERO,
                            0.0,
                            tint,
                        );
                    });
            }
        }
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
        let rect: Rectangle = transform.apply_to_rect(textbox.rect);
        let rect = rect
            .expand_vertical(rect.height * -Self::TEXTBOX_V_PADDING)
            .expand_horizontal(rect.width * -Self::TEXTBOX_H_PADDING)
            .round();

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
        let rect: Rectangle = transform.apply_to_rect(pattern.rect);

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
        let rect: Rectangle = transform.apply_to_rect(pattern.rect);
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
        let rect: Rectangle = transform.apply_to_rect(pattern.rect);

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
        let rect: Rectangle = transform.apply_to_rect(pattern.rect);

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
        let rect: Rectangle = transform.apply_to_rect(pattern.rect);

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
        let (render_dimensions, render_offset) = {
            // compute the base rect
            let mut rect = Rectangle::new(0.0, 0.0, 0.0, 0.0);
            for component in visual.components() {
                match component {
                    VisualComponent::Textbox(t) => {
                        rect = rect.bounding(&t.rect);
                    }

                    VisualComponent::Image(i) => match i.position_data {
                        ImagePosition::FitRect(r) => rect = rect.bounding(&r),
                        ImagePosition::Transform(transform) => {
                            if let Some(image_texture) =
                                visual_component_provider.get_or_load_image(self, &i.path)
                            {
                                let r = Rectangle::from_center_dimension(
                                    transform.position,
                                    Vector2::from_xy(
                                        image_texture.width() as f32,
                                        image_texture.height() as f32,
                                    )
                                    .scaled(transform.scale),
                                );

                                rect = rect.bounding(&r);
                            }
                        }
                    },

                    VisualComponent::Pattern(p) => rect = rect.bounding(&p.rect),
                }
            }

            // dimensions are a square centered on the origin, containing all components
            // and some padding
            let min: Vector2 = rect.min();
            let max: Vector2 = rect.max();
            let x = min.x.abs().max(max.x.abs()) * 2.0;
            let y = min.y.abs().max(max.y.abs()) * 2.0;

            let render_dimensions = Vector2::new(x, y);
            (
                render_dimensions,
                render_dimensions
                    .scaled(0.5)
                    .add(Vector2::from_xy(1.0, 1.0).scaled(Self::TEXTURE_PADDING as f32)),
            )
        };

        // create a render texture with the relevant bounds
        let width = render_dimensions.x.ceil() as u32 + Self::TEXTURE_PADDING * 2;
        let height = render_dimensions.y.ceil() as u32 + Self::TEXTURE_PADDING * 2;

        let mut texture = self
            .rl
            .load_render_texture(&self.thread, width, height)
            .expect("Could not create render texture.");

        let texture_transform = ImageTransform::<Vector2> {
            position: render_offset,
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
            position: render_offset.scale(magnification),
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
                            texture_transform.apply_to_image_position(&i.position_data),
                            &image,
                            &mut texture,
                            i.tint,
                        );
                        self.draw_texture(
                            magnified_transform.apply_to_image_position(&i.position_data),
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

    fn load_font_from_memory(
        &mut self,
        filetype: &str,
        bytes: &[u8],
        size: u32,
        characters: &str,
    ) -> Result<Self::Font, String> {
        match self.rl.load_font_from_memory(
            &self.thread,
            filetype,
            bytes,
            size as i32,
            Some(characters),
        ) {
            Ok(f) => Ok(f),
            Err(msg) => Err(format!("Failed to load font from memory: {msg}",)),
        }
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
            Err(msg) => Err(format!(
                "Failed to load font from '{}': {msg}",
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

#[derive(Debug)]
pub struct RaylibKeyboardReader<'a> {
    rl: &'a mut RaylibHandle,
}

impl<'a> IKeyboardReader for RaylibKeyboardReader<'a> {
    fn is_up_key_pressed(&mut self) -> bool {
        self.rl.is_key_down(KeyboardKey::KEY_UP)
    }

    fn is_down_key_pressed(&mut self) -> bool {
        self.rl.is_key_down(KeyboardKey::KEY_DOWN)
    }

    fn is_left_key_pressed(&mut self) -> bool {
        self.rl.is_key_down(KeyboardKey::KEY_LEFT)
    }

    fn is_right_key_pressed(&mut self) -> bool {
        self.rl.is_key_down(KeyboardKey::KEY_RIGHT)
    }

    fn is_backspace_key_pressed(&mut self) -> bool {
        self.rl.is_key_down(KeyboardKey::KEY_BACKSPACE)
    }

    fn is_delete_key_pressed(&mut self) -> bool {
        self.rl.is_key_down(KeyboardKey::KEY_DELETE)
    }

    fn is_enter_key_pressed(&mut self) -> bool {
        self.rl.is_key_down(KeyboardKey::KEY_ENTER)
    }

    fn poll_char_press(&mut self) -> Option<char> {
        self.rl.get_char_pressed()
    }
}

#[derive(Debug)]
pub struct RaylibRenderer<'a> {
    rl: &'a mut RaylibHandle,
    thread: &'a RaylibThread,
    draw_layer_pool: &'a mut Vec<RenderTexture2D>,
    output_texture: &'a mut RenderTexture2D,
    secs_since_cursor_moved: &'a mut f32,
}

impl<'a> RaylibRenderer<'a> {
    /// thickness for debug polygon lines
    const DEBUG_THICKNESS: f32 = 1.0;

    /// thickness for debug polygon line outlines
    const DEBUG_OUTLINE_THICKNESS: f32 = 3.0;

    // cheat buffer constants
    const CHEAT_SPACING: f32 = 1.0;
    const CHEAT_PADDING: f32 = 4.0;
    const CHEAT_BACKGROUND: Color = Color::BLACK;
    const CHEAT_BLINK_TIME: f32 = 0.5;
    const CHEAT_FOREGROUND: Color = Color {
        r: 0,
        g: 240,
        b: 0,
        a: 255,
    };

    /// how many seconds after inactivity to hide the cursor
    const HIDE_CURSOR_SECS: f32 = 5.0;

    fn draw_board(
        rl: &mut RaylibHandle,
        thread: &RaylibThread,
        draw_layer_pool: &mut Vec<RenderTexture2D>,
        board: &crate::board::BoardManager<
            Color,
            Rectangle,
            Vector2,
            ConvexHull<Vector2>,
            RenderTexture2D,
            Font,
        >,
        assets: &GlobalAssets<RenderTexture2D, Font>,
        output_texture: &mut RenderTexture2D,
    ) {
        let output_dimensions = board.dimensions();
        for _ in 0..board
            .draw_layer_count()
            .saturating_sub(draw_layer_pool.len())
        {
            draw_layer_pool.push(
                rl.load_render_texture(
                    thread,
                    output_dimensions.x() as u32,
                    output_dimensions.y() as u32,
                )
                .unwrap(),
            );
        }

        let mut pool_iter = draw_layer_pool.iter_mut();
        let mut draw_layers = BTreeMap::from_iter(board.draw_layer_iter().map(|l| {
            let mut layer = pool_iter.next().unwrap();
            rl.draw_texture_mode(thread, &mut layer, |mut painter| {
                painter.clear_background(Color::BLANK)
            });
            (l, layer)
        }));

        // collision layers for debug collision layer drawing
        let mut collision_layers = BTreeMap::new();
        let mut collision_layer_number = 0.0;

        for (texture, collider_opt, transform, layer) in board.texture_draw_iter() {
            let render_texture = draw_layers.get_mut(&layer).unwrap();
            let mut painter = rl.begin_texture_mode(thread, render_texture);

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

        rl.draw_texture_mode(thread, output_texture, |mut painter| {
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
                            while let (Some(p1), Some(p2)) = (point_iter.next(), point_iter.peek())
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

    fn draw_cheat_buffer(
        rl: &mut RaylibHandle,
        thread: &RaylibThread,
        buffer: &CheatBuffer,
        assets: &GlobalAssets<RenderTexture2D, Font>,
        output_texture: &mut RenderTexture2D,
    ) {
        let position = Vector2 {
            x: 0.0,
            y: output_texture.height() as f32
                - GlobalAssets::<RenderTexture2D, Font>::CHEAT_BUFFER_FONT_SIZE
                - 2.0 * Self::CHEAT_PADDING,
        };
        let mut painter = rl.begin_texture_mode(thread, output_texture);

        if buffer.command().is_empty() {
            return;
        }

        let text_dimensions = assets.cheat_buffer_font.measure_text(
            buffer.command(),
            GlobalAssets::<RenderTexture2D, Font>::CHEAT_BUFFER_FONT_SIZE as f32,
            Self::CHEAT_SPACING,
        );
        let cursor_position = assets
            .cheat_buffer_font
            .measure_text(
                &buffer.command()[0..buffer.cursor_index()],
                GlobalAssets::<RenderTexture2D, Font>::CHEAT_BUFFER_FONT_SIZE,
                Self::CHEAT_SPACING,
            )
            .with_y(0.0)
            .add(position);
        let cursor_dimensions = assets.cheat_buffer_font.measure_text(
            &"$",
            GlobalAssets::<RenderTexture2D, Font>::CHEAT_BUFFER_FONT_SIZE as f32,
            Self::CHEAT_SPACING,
        );

        let cursor_rect = Rectangle::from_position_dimension(cursor_position, cursor_dimensions);
        let big_cursor_rect = cursor_rect.expand_vertical(Self::CHEAT_PADDING);
        let background_rect = Rectangle::from_position_dimension(position, text_dimensions)
            .expand(Self::CHEAT_PADDING)
            .expand_right(big_cursor_rect.width);

        let should_draw_cursor =
            buffer.secs_since_input() % (Self::CHEAT_BLINK_TIME * 2.0) < Self::CHEAT_BLINK_TIME;

        painter.draw_rectangle_rec(background_rect, Self::CHEAT_BACKGROUND);
        if should_draw_cursor {
            painter.draw_rectangle_rec(big_cursor_rect, Self::CHEAT_FOREGROUND);
        }

        painter.draw_text_ex(
            &assets.cheat_buffer_font,
            buffer.command(),
            position,
            GlobalAssets::<RenderTexture2D, Font>::CHEAT_BUFFER_FONT_SIZE,
            Self::CHEAT_SPACING,
            Self::CHEAT_FOREGROUND,
        );

        if should_draw_cursor {
            painter.draw_rectangle_rec(cursor_rect, Self::CHEAT_FOREGROUND);
            if let Some(cursor_str) = buffer.cursor_str() {
                painter.draw_text_ex(
                    &assets.cheat_buffer_font,
                    cursor_str,
                    cursor_rect.position::<Vector2>(),
                    GlobalAssets::<RenderTexture2D, Font>::CHEAT_BUFFER_FONT_SIZE,
                    Self::CHEAT_SPACING,
                    Self::CHEAT_BACKGROUND,
                );
            }
        }
    }
}

impl<'a> IRenderer for RaylibRenderer<'a> {
    type Texture = RenderTexture2D;
    type Font = Font;
    type Color = Color;
    type Rectangle = Rectangle;
    type Vector2 = Vector2;
    type ConvexShape = ConvexHull<Vector2>;

    fn draw_app(
        &mut self,
        board: &BoardManager<
            Self::Color,
            Self::Rectangle,
            Self::Vector2,
            Self::ConvexShape,
            Self::Texture,
            Self::Font,
        >,
        cheat_buffer: &CheatBuffer,
        assets: &GlobalAssets<Self::Texture, Self::Font>,
    ) {
        // handle cursor hiding/showing
        if self.rl.get_mouse_delta() == Vector2::origin() {
            *self.secs_since_cursor_moved += 1.0 / TARGET_FRAME_RATE as f32;
        } else {
            *self.secs_since_cursor_moved = 0.0;
        }
        let should_show_ui = *self.secs_since_cursor_moved < Self::HIDE_CURSOR_SECS;
        match (self.rl.is_cursor_hidden(), should_show_ui) {
            (true, true) => self.rl.show_cursor(),
            (true, false) => (),
            (false, true) => (),
            (false, false) => self.rl.hide_cursor(),
        }

        // handle fullscreen
        if self.rl.is_key_released(KeyboardKey::KEY_F11) {
            self.rl.toggle_fullscreen();
            self.rl.set_window_position(100, 100);
        }

        // draw to screen
        self.rl
            .draw_texture_mode(self.thread, self.output_texture, |mut painter| {
                painter.clear_background(Color::BLANK);
            });
        Self::draw_board(
            self.rl,
            self.thread,
            self.draw_layer_pool,
            board,
            assets,
            self.output_texture,
        );
        Self::draw_cheat_buffer(
            self.rl,
            self.thread,
            cheat_buffer,
            assets,
            self.output_texture,
        );

        let render_rect = Rectangle::from_position_dimension(
            Vector2::origin(),
            Vector2::from_xy(
                self.rl.get_render_width() as f32,
                self.rl.get_render_height() as f32,
            ),
        );
        let src_rect =
            Rectangle::from_position_dimension(Vector2::origin(), board.dimensions().neg_y());
        let dst_rect = render_rect.centered_with_aspect_ratio(board.dimensions());

        self.rl.draw(self.thread, |mut painter| {
            painter.clear_background(Color::BLACK);

            painter.draw_texture_pro(
                &self.output_texture,
                src_rect,
                dst_rect,
                Vector2::origin(),
                0.0,
                Color::WHITE,
            );
        });
    }
}

#[derive(Debug)]
pub struct RaylibPlatform {
    rl: RaylibHandle,
    thread: RaylibThread,
    draw_layer_pool: Vec<RenderTexture2D>,
    output_texture: RenderTexture2D,
    secs_since_cursor_moved: f32,
}

impl RaylibPlatform {
    /// instantiates the new platform assuming the given board dimensions
    pub fn new(board_dimensions: Vector2) -> Self {
        callbacks::set_trace_log_callback(|level, msg| match level {
            TraceLogLevel::LOG_WARNING => println!("{msg}"),
            TraceLogLevel::LOG_ERROR => eprintln!("{msg}"),
            TraceLogLevel::LOG_FATAL => eprintln!("{msg}"),
            _ => (),
        })
        .expect("unable to set trace log level");
        let draw_layer_pool = Vec::new();
        let (mut rl, thread) = raylib::init()
            .always_run()
            .vsync()
            .title("Game Host")
            .size(
                board_dimensions.x() as i32 / 2,
                board_dimensions.y() as i32 / 2,
            )
            .fullscreen()
            .resizable()
            .build();
        rl.set_exit_key(None);
        rl.set_target_fps(TARGET_FRAME_RATE);
        let output_texture = rl
            .load_render_texture(
                &thread,
                board_dimensions.x as u32,
                board_dimensions.y as u32,
            )
            .expect("Unable to create board render layer");

        Self {
            rl,
            thread,
            draw_layer_pool,
            output_texture,
            secs_since_cursor_moved: 0.0,
        }
    }
}

impl IPlatform for RaylibPlatform {
    type Texture = RenderTexture2D;
    type Font = Font;
    type Color = Color;
    type Rectangle = Rectangle;
    type Vector2 = Vector2;
    type ConvexShape = ConvexHull<Vector2>;
    type KeyboardReader<'a> = RaylibKeyboardReader<'a>;
    type Renderer<'a> = RaylibRenderer<'a>;
    type TextureCompiler<'a> = RaylibTextureCompiler<'a>;

    fn should_close(&mut self) -> bool {
        self.rl.window_should_close()
    }

    fn toggle_fullscreen(&mut self) {
        self.rl.toggle_fullscreen();
        self.rl.set_window_position(100, 100);
    }

    fn delta_time(&mut self) -> f32 {
        1.0 / TARGET_FRAME_RATE as f32
    }

    fn renderer(&mut self) -> Self::Renderer<'_> {
        RaylibRenderer {
            rl: &mut self.rl,
            thread: &self.thread,
            draw_layer_pool: &mut self.draw_layer_pool,
            output_texture: &mut self.output_texture,
            secs_since_cursor_moved: &mut self.secs_since_cursor_moved,
        }
    }

    fn texture_compiler(&mut self) -> Self::TextureCompiler<'_> {
        RaylibTextureCompiler {
            rl: &mut self.rl,
            thread: &self.thread,
        }
    }

    fn keyboard_reader(&mut self) -> Self::KeyboardReader<'_> {
        RaylibKeyboardReader { rl: &mut self.rl }
    }

    fn run_game_picker(
        &mut self,
        games: &[crate::app::GameEntry<Self::Texture>],
    ) -> Option<std::path::PathBuf> {
        if games.is_empty() {
            // placeholder, delete later
            self.rl.draw(&self.thread, |mut p| {
                p.draw_text("No games to play!", 40, 40, 32, Color::BLACK);
            });

            return None;
        }

        /// game entries to draw per row
        const ENTRIES_PER_ROW: u32 = 3;

        /// aspect ratio of entries
        const ENTRY_ASPECT_RATIO: f32 = 4.0 / 3.0;

        /// padding between entries as proportion of screen width
        const PADDING_RATIO: f32 = 0.05;

        let max_entries_per_col = games.len().div_ceil(ENTRIES_PER_ROW as usize);
        let padding = self.rl.get_render_width() as f32 * PADDING_RATIO;
        let entry_width = (self.rl.get_render_width() as f32
            - PADDING_RATIO * (ENTRIES_PER_ROW + 1) as f32)
            / ENTRIES_PER_ROW as f32;
        let entry_dimensions = Vector2 {
            x: entry_width,
            y: entry_width / ENTRY_ASPECT_RATIO,
        };
        let entry_rect = Rectangle {
            x: padding,
            y: padding,
            width: (entry_dimensions.x + padding) * ENTRIES_PER_ROW as f32,
            height: (entry_dimensions.y + padding) * max_entries_per_col as f32,
        };

        let entry_rect_iter = entry_rect
            .evenly_spaced_rects(entry_dimensions.add(Vector2::from_xy(padding, padding)))
            .map(|r| {
                r.with_width(entry_dimensions.x)
                    .with_height(entry_dimensions.y)
            });

        if self.rl.is_key_released(KeyboardKey::KEY_F11) {
            self.rl.toggle_fullscreen();
            self.rl.set_window_position(100, 100);
        }

        for entry_rect in entry_rect_iter {}

        todo!()
    }
}
