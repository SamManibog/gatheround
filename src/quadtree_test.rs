use std::collections::HashMap;

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Resize, Stroke, Vec2};

use crate::{geometry::{IRect, IVec2}, quadtree::QuadTree};

impl IRect for egui::Rect {
    fn x(&self) -> f32 {
        self.min.x
    }

    fn y(&self) -> f32 {
        self.min.y
    }

    fn width(&self) -> f32 {
        egui::Rect::width(self)
    }

    fn height(&self) -> f32 {
        egui::Rect::height(self)
    }

    fn from_position_dimension<V1: IVec2, V2: IVec2>(position: V1, dimensions: V2) -> Self {
        egui::Rect::from_min_size((position.x(), position.y()).into(), (dimensions.x(), dimensions.y()).into())
    }
}

impl IVec2 for egui::Pos2 {
    fn x(&self) -> f32 {
        self.x
    }

    fn y(&self) -> f32 {
        self.y
    }

    fn from_xy(x: f32, y: f32) -> Self {
        Self::new(x, y)
    }
}

impl IVec2 for egui::Vec2 {
    fn x(&self) -> f32 {
        self.x
    }

    fn y(&self) -> f32 {
        self.y
    }

    fn from_xy(x: f32, y: f32) -> Self {
        Self::new(x, y)
    }
}

pub struct QuadTreeTest {
    quadtree: QuadTree<usize, egui::Rect>,
    rects: HashMap<usize, egui::Rect>,
    focused: usize,
    rect_list: Vec<usize>,
    draw_regions: bool,
    query_rect: egui::Rect,
}

impl QuadTreeTest {
    pub const QUADTREE_DIMENSIONS: egui::Vec2 = egui::Vec2::new(1000.0, 700.0);
    pub const RECT_MIN_SIZE: f32 = 40.0;
    pub const RECT_MAX_SIZE: f32 = Self::QUADTREE_DIMENSIONS.y - 10.0;
    pub const INITIAL_RECT: egui::Rect = egui::Rect {
        min: Pos2::new(20.0, 20.0),
        max: Pos2::new(70.0, 70.0),
    };

    pub fn run() {
        let native_options = eframe::NativeOptions::default();
        let _ = eframe::run_native(
            "QuadTree Test",
            native_options,
            Box::new(|cc| Ok(Box::new(QuadTreeTest::new(cc)))),
        );
    }

    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_global_style.
        // Restore app state using cc.storage (requires the "persistence" feature).
        // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
        // for e.g. egui::PaintCallback.
        
        let mut rects = HashMap::new();
        rects.insert(0, Self::INITIAL_RECT.clone()); 

        Self {
            quadtree: QuadTree::new(egui::Rect::from_min_size(Pos2::ZERO, Self::QUADTREE_DIMENSIONS)),
            rects,
            rect_list: vec![0],
            query_rect: Self::INITIAL_RECT,
            draw_regions: false,
            focused: 0,
        }
    }

    pub fn next_rect_id(&self) -> usize {
        for (idx, id) in self.rect_list.iter().enumerate() {
            if idx != *id {
                return idx;
            }
        }
        self.rect_list.len()
    }

    pub fn nearest_rect_id(&self, id: usize) -> usize {
        match self.rect_list.binary_search(&id) {
            Ok(x) | Err(x) => x
        }
    }

    /// adds a new rect and returns its id
    pub fn add_rect(&mut self) -> usize {
        let id = self.next_rect_id();

        self.rects.insert(id, egui::Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(50.0, 50.0)));
        self.quadtree.insert(id, &|e| { *self.rects.get(e).unwrap() });
        self.rect_list.insert(self.rect_list.binary_search(&id).expect_err("id should not be in add_rect"), id);

        id
    }
}

impl eframe::App for QuadTreeTest {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let mut old_focus_position = None;
        let mut to_delete_opt = None;
        let mut new_rect = None;
        let mut should_rebuid = false;

        egui::Panel::right("rect editor").show(ui, |ui| {
            // rectangle editor
            {
                let r = self.rects.get_mut(&self.focused).unwrap();
                ui.label(format!("Rect ID: {}", self.focused));

                old_focus_position = Some((r.center().x, r.center().y));

                let mut x = r.x();
                let mut y = r.y();
                let mut width = r.width();
                let mut height = r.height();

                ui.label("X");
                ui.add(egui::Slider::new(&mut x, 0.0..=Self::QUADTREE_DIMENSIONS.x - width));

                ui.label("Y");
                ui.add(egui::Slider::new(&mut y, 0.0..=Self::QUADTREE_DIMENSIONS.y - height));

                ui.label("Width");
                ui.add(egui::Slider::new(&mut width, Self::RECT_MIN_SIZE..=Self::RECT_MAX_SIZE));

                ui.label("Height");
                ui.add(egui::Slider::new(&mut height, Self::RECT_MIN_SIZE..=Self::RECT_MAX_SIZE));

                *r = egui::Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height));

                if ui.button("next").clicked() {
                    self.focused = (self.rects.len() - 1).min(self.focused + 1);
                }

                if ui.button("prev").clicked() {
                    self.focused = self.focused.saturating_sub(1);
                }

                if ui.button("delete").clicked() {
                    to_delete_opt = Some(self.focused);
                }
            }

            ui.separator();

            // query editor
            {
                ui.label("Query");

                let mut x = self.query_rect.x();
                let mut y = self.query_rect.y();
                let mut width = self.query_rect.width();
                let mut height = self.query_rect.height();

                ui.label("X");
                ui.add(egui::Slider::new(&mut x, 0.0..=Self::QUADTREE_DIMENSIONS.x - width));

                ui.label("Y");
                ui.add(egui::Slider::new(&mut y, 0.0..=Self::QUADTREE_DIMENSIONS.y - height));

                ui.label("Width");
                ui.add(egui::Slider::new(&mut width, Self::RECT_MIN_SIZE..=Self::RECT_MAX_SIZE));

                ui.label("Height");
                ui.add(egui::Slider::new(&mut height, Self::RECT_MIN_SIZE..=Self::RECT_MAX_SIZE));

                self.query_rect = egui::Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height));

                let contained = self.quadtree.query_range(self.query_rect, &|e| { *self.rects.get(e).unwrap() });
                let mut text = String::new();
                for c in contained {
                    text.push_str(&format!("{}, ", c));
                }
                ui.add(egui::TextEdit::multiline(&mut text).interactive(false));
            }

            ui.separator();

            // tree editor
            {
                if ui.button("New Rect").clicked() {
                    new_rect = Some(self.add_rect());
                }

                ui.checkbox(&mut self.draw_regions, "Draw QuadTree Regions");

                should_rebuid = ui.button("Rebuild Quadtree").clicked();
            }
        });

        // update quadtree with new rect position
        if let Some(pos) = old_focus_position {
            self.quadtree.extract_on_point(pos, &|e| { *e == self.focused });
            self.quadtree.insert(self.focused, &|e| { *self.rects.get(e).unwrap() });
        }

        // draw quadtree
        egui::CentralPanel::default().show(ui, |ui| {
            ui.with_layout( egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                Resize::default().fixed_size(Self::QUADTREE_DIMENSIONS).show(ui, |ui| {
                    let offset = ui.max_rect().min.to_vec2();
                    // let offset = ui.response().rect.min.to_vec2();
                    let painter = ui.painter();

                    // draw quadtree
                    self.quadtree.debug_draw(
                        &|e| { *self.rects.get(e).unwrap() },
                        &mut |aabb, entry| {
                            if let Some(e) = entry {
                                let hue = *e as f32 / self.rects.len() as f32;

                                let stroke = if *e == self.focused {
                                    Stroke::new(2.0, Color32::WHITE)
                                } else {
                                    Stroke::NONE
                                };

                                let rect = aabb.clone().translate(offset);

                                painter.rect(
                                    rect,
                                    0,
                                    egui::epaint::Hsva::new(hue, 1.0, 1.0, 1.0),
                                    stroke,
                                    egui::StrokeKind::Inside
                                );
                                painter.text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    e,
                                    FontId::monospace(24.0),
                                    egui::epaint::Hsva::new((hue + 0.5) % 1.0, 1.0, 1.0, 1.0).into()
                                );
                            } if self.draw_regions {
                                painter.rect_stroke(
                                    aabb.clone().translate(offset),
                                    0,
                                    Stroke::new(2.0, Color32::WHITE),
                                    egui::StrokeKind::Middle
                                );
                            }
                        }
                    );

                    // draw query rect
                    painter.rect(
                        self.query_rect.clone().translate(offset),
                        0,
                        Color32::from_black_alpha(70),
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 255, 255, 70)),
                        egui::StrokeKind::Inside
                    );

                })
            });
        });

        if let Some(rect) = new_rect {
            self.focused = rect;
        }

        if let Some(to_delete) = to_delete_opt && self.rect_list.len() > 1 {
            let delete_pos = self.rects.get(&to_delete).unwrap().center();
            self.quadtree.extract_on_point(delete_pos, &|e| { *e == self.focused });
            self.rects.remove(&to_delete);

            if self.focused == to_delete {
                self.focused = self.nearest_rect_id(to_delete);
            }

            self.rect_list.remove(to_delete);
        }

        if should_rebuid {
            self.quadtree = QuadTree::new(egui::Rect::from_min_size(Pos2::ZERO, Self::QUADTREE_DIMENSIONS));
            for rect in &self.rect_list {
                self.quadtree.insert(*rect, &|e| { *self.rects.get(e).unwrap() });
            }
        }
    }
}
