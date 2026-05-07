//! Fractal clock screensaver for [egui](https://github.com/emilk/egui).
//!
//! Draws three clock hands (seconds, minutes, hours) from the centre of the
//! screen.  Each frame the tips of the seconds and minutes hands recursively
//! sprout two child branches, rotated by the angular difference between those
//! hands and the hour hand.  The result is a continuously-morphing fractal
//! tree whose shape encodes the current time.
//!
//! Rendering is capped at 30 FPS. If the hardware cannot sustain that rate,
//! frames are painted as fast as possible without any artificial delay.
//!
//! # Usage
//!
//! ```rust,no_run
//! use egui_screensaver_fractal_clock::FractalClockBackground;
//!
//! struct MyApp {
//!     fractal_clock: FractalClockBackground,
//! }
//!
//! impl eframe::App for MyApp {
//!     fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
//!         // Call paint once per frame before drawing any UI windows so the
//!         // screensaver sits on the background layer behind everything else.
//!         self.fractal_clock.paint(ctx);
//!     }
//! }
//! ```

use core::f32::consts::TAU;

use egui::{
    Color32, Context, LayerId, Painter, Pos2, Rect, Shape, Vec2, emath::RectTransform, pos2,
};

/// Fractal clock screensaver state.
///
/// Create one instance (e.g. as a field of your `eframe::App` struct) and call
/// [`FractalClockBackground::paint`] every frame from your `update` method.
#[derive(Debug)]
pub struct FractalClockBackground {
    /// Overall scale of the clock relative to the window.  Smaller values
    /// zoom out and show more of the fractal; larger values zoom in.
    zoom: f32,
    /// Stroke width of the three primary clock hands.
    start_line_width: f32,
    /// Maximum recursion depth.  Higher values produce more branches but cost
    /// more CPU per frame.
    depth: usize,
    /// Length of each branch relative to its parent.  Values close to 1.0
    /// produce long, sprawling fractals; values near 0.5 stay compact.
    length_factor: f32,
    /// Luminance multiplier applied at each recursion level.  Branches become
    /// darker with depth and iteration stops when luminance rounds to 0.
    luminance_factor: f32,
    /// Stroke width multiplier applied at each recursion level so deeper
    /// branches are drawn thinner.
    width_factor: f32,
}

impl Default for FractalClockBackground {
    fn default() -> Self {
        Self {
            zoom: 0.25,
            start_line_width: 2.5,
            depth: 9,
            length_factor: 0.8,
            luminance_factor: 0.8,
            width_factor: 0.9,
        }
    }
}

impl FractalClockBackground {
    /// Paint the screensaver onto the egui background layer for this frame.
    ///
    /// Call this once per frame **before** drawing any UI panels or windows so
    /// the animation appears behind all other content.
    ///
    /// Repaints are capped at 30 FPS; if the hardware cannot sustain that rate
    /// the clock animates as fast as possible without any artificial delay.
    pub fn paint(&mut self, ctx: &Context) {
        // A single clock hand: its length, angle (radians from 12 o'clock),
        // and the corresponding direction vector.
        struct Hand {
            length: f32,
            angle: f32,
            vec: Vec2,
        }

        impl Hand {
            fn from_length_angle(length: f32, angle: f32) -> Self {
                Self {
                    length,
                    angle,
                    vec: length * Vec2::angled(angle),
                }
            }
        }

        // `ctx.input.time` is seconds since the application started.  We
        // derive clock-hand angles by taking the fractional part within each
        // hand's period and mapping [0, period) → [−π/2, 3π/2) so that 12
        // o'clock points straight up (negative Y in screen space).
        let time = ctx.input(|input| input.time);

        // Request a repaint after ~33 ms to cap animation at 30 FPS.
        // If the frame takes longer than that, egui repaints immediately.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(1.0 / 30.0));

        let angle_from_period =
            |period| TAU * (time.rem_euclid(period) / period) as f32 - TAU / 4.0;

        let hands = [
            Hand::from_length_angle(self.length_factor, angle_from_period(60.0)), // seconds
            Hand::from_length_angle(self.length_factor, angle_from_period(60.0 * 60.0)), // minutes
            Hand::from_length_angle(0.5, angle_from_period(12.0 * 60.0 * 60.0)),  // hours
        ];

        let rect = ctx.viewport_rect();
        let painter = Painter::new(ctx.clone(), LayerId::background(), rect);

        // Map from a logical coordinate system centred at (0, 0) with the
        // clock's square proportions to actual screen pixels.
        let to_screen = RectTransform::from_to(
            Rect::from_center_size(Pos2::ZERO, rect.square_proportions() / self.zoom),
            rect,
        );

        // Collect all line segments into a single Vec and submit them in one
        // `painter.extend` call to minimise draw-call overhead.
        let mut shapes: Vec<Shape> = Vec::new();

        let mut paint_line = |points: [Pos2; 2], color: Color32, width: f32| {
            let line = [to_screen * points[0], to_screen * points[1]];
            // Skip lines that are entirely off-screen to avoid submitting
            // degenerate geometry to the renderer.
            if rect.intersects(Rect::from_two_pos(line[0], line[1])) {
                shapes.push(Shape::line_segment(line, (width, color)));
            }
        };

        // The two "rotors" encode how each child branch is rotated and scaled
        // relative to its parent.  Each rotor is a scaled Rot2 derived from
        // the angular offset between the seconds/minutes hand and the hour
        // hand.  Adding TAU/2 flips direction so branches point inward.
        let hand_rotations = [
            hands[0].angle - hands[2].angle + TAU / 2.0,
            hands[1].angle - hands[2].angle + TAU / 2.0,
        ];

        let hand_rotors = [
            hands[0].length * egui::emath::Rot2::from_angle(hand_rotations[0]),
            hands[1].length * egui::emath::Rot2::from_angle(hand_rotations[1]),
        ];

        // Draw the three primary clock hands at full brightness.
        let mut width = self.start_line_width;
        let mut nodes: Vec<Node> = Vec::new();
        let center = pos2(0.0, 0.0);

        for (i, hand) in hands.iter().enumerate() {
            let end = center + hand.vec;
            paint_line([center, end], Color32::from_additive_luminance(255), width);

            // Only the seconds and minutes hand tips spawn fractal branches;
            // the hour hand is short and acts as the rotation reference only.
            if i < 2 {
                nodes.push(Node {
                    pos: end,
                    dir: hand.vec,
                });
            }
        }

        // Iteratively expand the fractal tree depth-first.  Each generation
        // replaces the current set of nodes with two children per node (one
        // per rotor), drawing a line from parent to child.
        let mut luminance = 0.7_f32;
        let mut new_nodes: Vec<Node> = Vec::new();

        for _ in 0..self.depth {
            new_nodes.clear();
            new_nodes.reserve(nodes.len() * 2);

            luminance *= self.luminance_factor;
            width *= self.width_factor;

            // Stop early once branches are too dim to be visible.
            let luminance_u8 = (255.0 * luminance).round() as u8;
            if luminance_u8 == 0 {
                break;
            }

            let color = Color32::from_additive_luminance(luminance_u8);

            for &rotor in &hand_rotors {
                for node in &nodes {
                    // Apply the rotor: rotate and scale `dir` to get the new
                    // branch direction, then step forward from the parent tip.
                    let new_dir = rotor * node.dir;
                    let next_node = Node {
                        pos: node.pos + new_dir,
                        dir: new_dir,
                    };
                    paint_line([node.pos, next_node.pos], color, width);
                    new_nodes.push(next_node);
                }
            }

            // Swap buffers so the next iteration works on the newly generated
            // nodes without extra allocation.
            std::mem::swap(&mut nodes, &mut new_nodes);
        }

        painter.extend(shapes);
    }
}

/// A single node in the fractal tree: a screen position and a direction
/// vector that describes how the next generation of branches will grow.
#[derive(Clone, Copy)]
struct Node {
    pos: Pos2,
    dir: Vec2,
}
