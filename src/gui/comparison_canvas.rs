//! Shared image-space navigation for the enlarged comparison view.

use std::sync::OnceLock;

use canvas::{Action, Canvas, Event, Frame, Geometry, Program};
use iced::widget::{canvas, image};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use super::comparison::split_column;

const DIVIDER_HIT_RADIUS: f32 = 12.0;
const MAX_ZOOM: f32 = 20.0;

pub(super) fn view<'a, Message: Clone + 'a>(
    handle: image::Handle,
    dimensions: (u32, u32),
    divider: Option<f32>,
    reset_id: u64,
    on_divider: fn(f32) -> Message,
) -> Element<'a, Message> {
    Canvas::new(ComparisonCanvas {
        handle,
        dimensions,
        divider,
        reset_id,
        on_divider,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct ComparisonCanvas<Message> {
    handle: image::Handle,
    dimensions: (u32, u32),
    divider: Option<f32>,
    reset_id: u64,
    on_divider: fn(f32) -> Message,
}

impl<Message> ComparisonCanvas<Message> {
    fn visible_divider(&self) -> Option<f32> {
        self.divider.map(|fraction| {
            let width = self.dimensions.0.max(1) as f32;

            // The composited wipe selects whole source pixels. Keep the handle
            // aligned with that boundary even at high magnification.
            split_column(self.dimensions.0, fraction) as f32 / width
        })
    }
}

#[derive(Default)]
struct State {
    reset_id: Option<u64>,
    viewport: Viewport,
    drag: Option<Drag>,
    pointer: Option<Point>,
}

impl State {
    fn viewport(&self, reset_id: u64) -> Viewport {
        if self.reset_id == Some(reset_id) {
            self.viewport
        } else {
            Viewport::default()
        }
    }

    fn reset_if_changed(&mut self, reset_id: u64) {
        if self.reset_id != Some(reset_id) {
            self.reset_id = Some(reset_id);
            self.viewport = Viewport::default();
            self.drag = None;
        }
    }
}

#[derive(Clone, Copy)]
enum Drag {
    Pan { previous: Point },
    Divider,
}

#[derive(Clone, Copy)]
struct Viewport {
    zoom: f32,
    pan: Vector,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vector::ZERO,
        }
    }
}

impl Viewport {
    fn image_rect(self, canvas_size: Size, dimensions: (u32, u32)) -> Rectangle {
        let width = dimensions.0.max(1) as f32;
        let height = dimensions.1.max(1) as f32;
        let fit = (canvas_size.width / width).min(canvas_size.height / height);
        let size = Size::new(width * fit * self.zoom, height * fit * self.zoom);
        let pan = self.clamped_pan(canvas_size, size);

        Rectangle::new(
            Point::new(
                (canvas_size.width - size.width) / 2.0 + pan.x,
                (canvas_size.height - size.height) / 2.0 + pan.y,
            ),
            size,
        )
    }

    fn clamped_pan(self, canvas_size: Size, image_size: Size) -> Vector {
        let horizontal_limit = ((image_size.width - canvas_size.width) / 2.0).max(0.0);
        let vertical_limit = ((image_size.height - canvas_size.height) / 2.0).max(0.0);

        Vector::new(
            self.pan.x.clamp(-horizontal_limit, horizontal_limit),
            self.pan.y.clamp(-vertical_limit, vertical_limit),
        )
    }

    fn clamp_pan(&mut self, canvas_size: Size, dimensions: (u32, u32)) {
        let image_size = self.image_rect(canvas_size, dimensions).size();
        self.pan = self.clamped_pan(canvas_size, image_size);
    }

    fn zoom_at(&mut self, cursor: Point, factor: f32, size: Size, dimensions: (u32, u32)) {
        self.clamp_pan(size, dimensions);

        let previous = self.image_rect(size, dimensions);
        let zoom = (self.zoom * factor).clamp(1.0, MAX_ZOOM);
        let ratio = zoom / self.zoom;
        let next_size = Size::new(previous.width * ratio, previous.height * ratio);
        let next_origin = Point::new(
            cursor.x - (cursor.x - previous.x) * ratio,
            cursor.y - (cursor.y - previous.y) * ratio,
        );

        self.zoom = zoom;
        self.pan = Vector::new(
            next_origin.x - (size.width - next_size.width) / 2.0,
            next_origin.y - (size.height - next_size.height) / 2.0,
        );
        self.clamp_pan(size, dimensions);
    }
}

fn divider_fraction(cursor: Point, image_rect: Rectangle) -> f32 {
    if image_rect.width <= 0.0 {
        return 0.5;
    }

    ((cursor.x - image_rect.x) / image_rect.width).clamp(0.0, 1.0)
}

fn near_divider(cursor: Point, image_rect: Rectangle, divider: f32) -> bool {
    let divider_x = image_rect.x + divider * image_rect.width;

    cursor.y >= image_rect.y
        && cursor.y <= image_rect.y + image_rect.height
        && (cursor.x - divider_x).abs() <= DIVIDER_HIT_RADIUS
}

impl<Message> Program<Message> for ComparisonCanvas<Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        state.reset_if_changed(self.reset_id);
        state.viewport.clamp_pan(bounds.size(), self.dimensions);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                // Iced supplies the final cursor position to every event in a
                // batch. Use chronological motion events for narrow hit areas,
                // while respecting an unavailable cursor or an overlay above.
                let current = cursor.position()?;
                let position = state.pointer.unwrap_or(current);

                if !bounds.contains(position) {
                    return None;
                }

                let position = position - Vector::new(bounds.x, bounds.y);
                let image_rect = state.viewport.image_rect(bounds.size(), self.dimensions);

                state.drag = Some(
                    if self
                        .visible_divider()
                        .is_some_and(|divider| near_divider(position, image_rect, divider))
                    {
                        Drag::Divider
                    } else {
                        Drag::Pan { previous: position }
                    },
                );

                Some(Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                state.pointer = Some(*position);

                let position = *position - Vector::new(bounds.x, bounds.y);

                match state.drag? {
                    Drag::Divider if self.divider.is_some() => {
                        let image_rect = state.viewport.image_rect(bounds.size(), self.dimensions);
                        let fraction = divider_fraction(position, image_rect);

                        Some(Action::publish((self.on_divider)(fraction)).and_capture())
                    }
                    Drag::Divider => {
                        state.drag = None;
                        Some(Action::request_redraw())
                    }
                    Drag::Pan { previous } => {
                        state.viewport.pan += position - previous;
                        state.viewport.clamp_pan(bounds.size(), self.dimensions);
                        state.drag = Some(Drag::Pan { previous: position });

                        Some(Action::request_redraw().and_capture())
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => state
                .drag
                .take()
                .map(|_| Action::request_redraw().and_capture()),
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.pointer = None;
                None
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let position = cursor.position_in(bounds)?;
                let exponent = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 0.15,
                    mouse::ScrollDelta::Pixels { y, .. } => y * 0.002,
                };

                state.viewport.zoom_at(
                    position,
                    exponent.clamp(-3.0, 3.0).exp(),
                    bounds.size(),
                    self.dimensions,
                );

                Some(Action::request_redraw().and_capture())
            }
            // Losing focus should not leave the next pointer movement dragging.
            Event::Window(iced::window::Event::Unfocused) => {
                state.drag = None;
                state.pointer = None;
                Some(Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let canvas_bounds = Rectangle::with_size(bounds.size());
        let image_rect = state
            .viewport(self.reset_id)
            .image_rect(bounds.size(), self.dimensions);

        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::from_rgb8(25, 26, 29));

        if let Some(visible) = image_rect.intersection(&canvas_bounds) {
            // Keep background and tiles in one frame: Iced flushes parent
            // meshes after clipped child meshes, which would hide the tiles.
            draw_checkerboard(&mut frame, visible);

            frame.with_clip(visible, |frame| {
                frame.draw_image(
                    image_rect,
                    canvas::Image::new(self.handle.clone())
                        .filter_method(image::FilterMethod::Nearest),
                );
            });

            if let Some(divider) = self.visible_divider() {
                frame.with_clip(canvas_bounds, |frame| {
                    draw_divider(frame, image_rect, visible, divider);
                });
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.reset_id == Some(self.reset_id) {
            match state.drag {
                Some(Drag::Divider) => return mouse::Interaction::ResizingHorizontally,
                Some(Drag::Pan { .. }) => return mouse::Interaction::Grabbing,
                None => {}
            }
        }

        let Some(position) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };

        let image_rect = state
            .viewport(self.reset_id)
            .image_rect(bounds.size(), self.dimensions);

        if self
            .visible_divider()
            .is_some_and(|divider| near_divider(position, image_rect, divider))
        {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::Grab
        }
    }
}

fn draw_checkerboard(frame: &mut Frame, visible: Rectangle) {
    const TILE: f32 = 16.0;

    let first_column = (visible.x / TILE).floor() as i32;
    let last_column = ((visible.x + visible.width) / TILE).ceil() as i32;
    let first_row = (visible.y / TILE).floor() as i32;
    let last_row = ((visible.y + visible.height) / TILE).ceil() as i32;

    for row in first_row..last_row {
        for column in first_column..last_column {
            let shade = if (row + column) % 2 == 0 { 72 } else { 104 };
            let tile = Rectangle::new(
                Point::new(column as f32 * TILE, row as f32 * TILE),
                Size::new(TILE, TILE),
            );

            if let Some(tile) = tile.intersection(&visible) {
                frame.fill_rectangle(
                    tile.position(),
                    tile.size(),
                    Color::from_rgb8(shade, shade, shade),
                );
            }
        }
    }
}

fn draw_divider(frame: &mut Frame, image_rect: Rectangle, visible: Rectangle, fraction: f32) {
    static LINE: OnceLock<image::Handle> = OnceLock::new();
    static KNOB: OnceLock<image::Handle> = OnceLock::new();

    // Canvas renders vector meshes before images, regardless of call order.
    // Raster handles keep the divider above the opaque comparison image.
    let line = LINE.get_or_init(|| {
        image::Handle::from_rgba(3, 1, vec![0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255])
    });

    let knob = KNOB.get_or_init(|| {
        let mut pixels = vec![0; 24 * 24 * 4];

        for row in 0..24 {
            for column in 0..24 {
                let distance =
                    ((column as f32 - 11.5).powi(2) + (row as f32 - 11.5).powi(2)).sqrt();
                let pixel = &mut pixels[(row * 24 + column) * 4..(row * 24 + column + 1) * 4];

                if distance <= 11.5 {
                    let shade = if distance <= 9.5 { 255 } else { 0 };
                    pixel.copy_from_slice(&[shade, shade, shade, 255]);
                }
            }
        }

        image::Handle::from_rgba(24, 24, pixels)
    });

    let divider_x = image_rect.x + fraction * image_rect.width;

    frame.draw_image(
        Rectangle::new(
            Point::new(divider_x - 1.5, visible.y),
            Size::new(3.0, visible.height),
        ),
        canvas::Image::new(line.clone()).filter_method(image::FilterMethod::Nearest),
    );
    frame.draw_image(
        Rectangle::new(
            Point::new(divider_x - 12.0, visible.center_y() - 12.0),
            Size::new(24.0, 24.0),
        ),
        canvas::Image::new(knob.clone()).filter_method(image::FilterMethod::Nearest),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_image_and_divider_respect_letterboxing() {
        let image_rect = Viewport::default().image_rect(Size::new(600.0, 400.0), (100, 100));

        assert_eq!(
            image_rect,
            Rectangle::new(Point::new(100.0, 0.0), Size::new(400.0, 400.0))
        );
        assert_eq!(divider_fraction(Point::new(200.0, 0.0), image_rect), 0.25);
        assert_eq!(divider_fraction(Point::new(0.0, 0.0), image_rect), 0.0);
        assert_eq!(divider_fraction(Point::new(600.0, 0.0), image_rect), 1.0);
        assert!(near_divider(Point::new(300.0, 250.0), image_rect, 0.5));
        assert!(!near_divider(Point::new(313.0, 250.0), image_rect, 0.5));
    }

    #[test]
    fn zoom_keeps_cursor_over_same_image_coordinate() {
        let mut viewport = Viewport::default();
        let size = Size::new(600.0, 400.0);
        let cursor = Point::new(250.0, 150.0);
        let initial_fraction = divider_fraction(cursor, viewport.image_rect(size, (100, 100)));

        viewport.zoom_at(cursor, 2.0, size, (100, 100));

        let image_rect = viewport.image_rect(size, (100, 100));
        assert_eq!(divider_fraction(cursor, image_rect), initial_fraction);
        assert_eq!((cursor.y - image_rect.y) / image_rect.height, 0.375);
    }

    #[test]
    fn navigation_bounds_prevent_losing_the_image() {
        let mut viewport = Viewport::default();
        let size = Size::new(600.0, 400.0);

        viewport.zoom_at(Point::new(300.0, 200.0), 100.0, size, (100, 100));
        assert_eq!(viewport.zoom, MAX_ZOOM);

        viewport.pan = Vector::new(100_000.0, -100_000.0);
        viewport.clamp_pan(size, (100, 100));

        let image_rect = viewport.image_rect(size, (100, 100));
        assert_eq!(image_rect.x, 0.0);
        assert_eq!(image_rect.y + image_rect.height, size.height);

        viewport.zoom_at(Point::new(300.0, 200.0), 0.001, size, (100, 100));
        assert_eq!(viewport.zoom, 1.0);
        assert_eq!(viewport.pan, Vector::ZERO);
    }

    #[test]
    fn divider_drag_maps_through_zoom_pan_and_clamps_outside() {
        let viewport = Viewport {
            zoom: 2.0,
            pan: Vector::new(70.0, -30.0),
        };

        let image_rect = viewport.image_rect(Size::new(600.0, 400.0), (100, 100));
        let divider_x = image_rect.x + 0.75 * image_rect.width;

        assert!(near_divider(Point::new(divider_x, 200.0), image_rect, 0.75));
        assert_eq!(
            divider_fraction(Point::new(divider_x, 200.0), image_rect),
            0.75
        );
        assert_eq!(
            divider_fraction(Point::new(-1_000.0, 200.0), image_rect),
            0.0
        );
        assert_eq!(
            divider_fraction(Point::new(2_000.0, 200.0), image_rect),
            1.0
        );
    }

    #[test]
    fn reset_changes_drawing_before_the_next_event_and_cancels_drag() {
        let mut state = State {
            reset_id: Some(1),
            viewport: Viewport {
                zoom: 3.0,
                pan: Vector::new(10.0, 15.0),
            },
            drag: Some(Drag::Divider),
            ..State::default()
        };

        assert_eq!(state.viewport(2).zoom, 1.0);
        assert_eq!(state.viewport(2).pan, Vector::ZERO);

        state.reset_if_changed(2);
        assert_eq!(state.viewport.zoom, 1.0);
        assert_eq!(state.viewport.pan, Vector::ZERO);
        assert!(state.drag.is_none());
    }

    #[test]
    fn mouse_release_outside_canvas_ends_drag() {
        let program = ComparisonCanvas {
            handle: image::Handle::from_rgba(1, 1, vec![0; 4]),
            dimensions: (1, 1),
            divider: Some(0.5),
            reset_id: 1,
            on_divider: |fraction| fraction,
        };

        let mut state = State {
            reset_id: Some(1),
            drag: Some(Drag::Divider),
            ..State::default()
        };

        let action = program.update(
            &mut state,
            &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Rectangle::with_size(Size::new(100.0, 100.0)),
            mouse::Cursor::Available(Point::new(500.0, 500.0)),
        );

        assert!(action.is_some());
        assert!(state.drag.is_none());
    }

    #[test]
    fn divider_events_use_canvas_origin_and_deliver_clamped_fraction() {
        let program = ComparisonCanvas {
            handle: image::Handle::from_rgba(1, 1, vec![0; 4]),
            dimensions: (100, 100),
            divider: Some(0.5),
            reset_id: 1,
            on_divider: |fraction| fraction,
        };

        let mut state = State::default();
        let bounds = Rectangle::new(Point::new(100.0, 60.0), Size::new(600.0, 400.0));
        let cursor = mouse::Cursor::Available(Point::new(400.0, 260.0));

        program.update(
            &mut state,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            cursor,
        );

        assert!(matches!(state.drag, Some(Drag::Divider)));

        let action = program
            .update(
                &mut state,
                &Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(1_000.0, 260.0),
                }),
                bounds,
                mouse::Cursor::Unavailable,
            )
            .unwrap();

        assert_eq!(action.into_inner().0, Some(1.0));
    }

    #[test]
    fn displayed_divider_tracks_the_composited_pixel_boundary() {
        let mut program = ComparisonCanvas {
            handle: image::Handle::from_rgba(1, 1, vec![0; 4]),
            dimensions: (2, 1),
            divider: Some(0.74),
            reset_id: 1,
            on_divider: |fraction| fraction,
        };

        assert_eq!(program.visible_divider(), Some(0.5));

        program.divider = Some(1.0);
        assert_eq!(program.visible_divider(), Some(1.0));
    }

    #[test]
    fn batched_drag_uses_the_pointer_at_press_time() {
        let program = ComparisonCanvas {
            handle: image::Handle::from_rgba(1, 1, vec![0; 4]),
            dimensions: (100, 100),
            divider: Some(0.5),
            reset_id: 1,
            on_divider: |fraction| fraction,
        };

        let mut state = State::default();
        let bounds = Rectangle::new(Point::new(100.0, 60.0), Size::new(600.0, 400.0));
        let start = Point::new(400.0, 260.0);
        let end = Point::new(500.0, 260.0);
        let cursor = mouse::Cursor::Available(end);

        // The whole batch receives its final cursor position, including the
        // earlier press. The preceding motion event retains the true start.
        program.update(
            &mut state,
            &Event::Mouse(mouse::Event::CursorMoved { position: start }),
            bounds,
            cursor,
        );
        program.update(
            &mut state,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            cursor,
        );

        assert!(matches!(state.drag, Some(Drag::Divider)));

        let action = program
            .update(
                &mut state,
                &Event::Mouse(mouse::Event::CursorMoved { position: end }),
                bounds,
                cursor,
            )
            .unwrap();

        assert_eq!(action.into_inner().0, Some(0.75));
    }

    #[test]
    fn pointer_history_respects_overlays_and_window_departure() {
        let program = ComparisonCanvas {
            handle: image::Handle::from_rgba(1, 1, vec![0; 4]),
            dimensions: (100, 100),
            divider: Some(0.5),
            reset_id: 1,
            on_divider: |fraction| fraction,
        };

        let point = Point::new(50.0, 50.0);
        let bounds = Rectangle::with_size(Size::new(100.0, 100.0));
        let mut state = State {
            pointer: Some(point),
            ..State::default()
        };

        for cursor in [mouse::Cursor::Unavailable, mouse::Cursor::Levitating(point)] {
            assert!(
                program
                    .update(
                        &mut state,
                        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                        bounds,
                        cursor,
                    )
                    .is_none()
            );
            assert!(state.drag.is_none());
        }

        state.drag = Some(Drag::Divider);
        program.update(
            &mut state,
            &Event::Mouse(mouse::Event::CursorLeft),
            bounds,
            mouse::Cursor::Unavailable,
        );

        assert!(state.pointer.is_none());
        assert!(matches!(state.drag, Some(Drag::Divider)));

        program.update(
            &mut state,
            &Event::Window(iced::window::Event::Unfocused),
            bounds,
            mouse::Cursor::Unavailable,
        );

        assert!(state.drag.is_none());
    }
}
