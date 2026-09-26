//! Keyboard focus for controls that Iced does not include in its focus traversal.

use std::ops::RangeInclusive;

use iced::{Element, Theme, keyboard, widget};
use iced_runtime::core::{
    Clipboard, Event, Layout, Length, Rectangle, Renderer as CoreRenderer, Shell, Size, Vector,
    Widget, layout, mouse, overlay, renderer, touch,
    widget::{Operation, Tree, operation::Focusable, tree},
};

pub fn button<'a, Message: Clone + 'a, Renderer: CoreRenderer + 'a>(
    button: widget::Button<'a, Message, Theme, Renderer>,
    on_press: Option<Message>,
) -> Element<'a, Message, Theme, Renderer> {
    let enabled = on_press.is_some();
    let content = button.on_press_maybe(on_press.clone()).into();

    Element::new(KeyboardControl {
        content,
        enabled,
        on_key: Box::new(move |event| match event {
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space),
                modifiers,
                repeat: false,
                ..
            } if modifiers.is_empty() => on_press.clone(),
            _ => None,
        }),
    })
}

pub fn slider<'a, Message: Clone + 'a, Renderer: CoreRenderer + 'a>(
    range: RangeInclusive<f32>,
    value: f32,
    step: f32,
    on_change: impl Fn(f32) -> Message + Clone + 'a,
) -> Element<'a, Message, Theme, Renderer> {
    let content = widget::Slider::new(range.clone(), value, on_change.clone())
        .step(step)
        .into();

    Element::new(KeyboardControl {
        content,
        enabled: true,
        on_key: Box::new(move |event| {
            let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
                return None;
            };

            if !modifiers.is_empty() {
                return None;
            }

            let offset = match key {
                keyboard::Key::Named(
                    keyboard::key::Named::ArrowLeft | keyboard::key::Named::ArrowDown,
                ) => -step,
                keyboard::Key::Named(
                    keyboard::key::Named::ArrowRight | keyboard::key::Named::ArrowUp,
                ) => step,
                _ => return None,
            };

            Some(on_change(
                (value + offset).clamp(*range.start(), *range.end()),
            ))
        }),
    })
}

#[derive(Default)]
struct State {
    focused: bool,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }

    fn focus(&mut self) {
        self.focused = true;
    }

    fn unfocus(&mut self) {
        self.focused = false;
    }
}

struct KeyboardControl<'a, Message, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    enabled: bool,
    on_key: Box<dyn Fn(&keyboard::Event) -> Option<Message> + 'a>,
}

impl<Message, Renderer: CoreRenderer> Widget<Message, Theme, Renderer>
    for KeyboardControl<'_, Message, Renderer>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.content.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        if !self.enabled {
            tree.state.downcast_mut::<State>().unfocus();
        }

        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.enabled {
            operation.focusable(None, layout.bounds(), tree.state.downcast_mut::<State>());
        }

        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let pressed_inside = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Some(cursor.is_over(layout.bounds()))
            }
            Event::Touch(touch::Event::FingerPressed { position, .. }) => {
                Some(layout.bounds().contains(*position))
            }
            _ => None,
        };

        if let Some(pressed_inside) = pressed_inside {
            let focused = self.enabled && pressed_inside && !shell.is_event_captured();

            if state.focused != focused {
                state.focused = focused;
                shell.request_redraw();
            }
        }

        if let Event::Keyboard(event @ keyboard::Event::KeyPressed { .. }) = event {
            if self.enabled
                && state.focused
                && !shell.is_event_captured()
                && let Some(message) = (self.on_key)(event)
            {
                shell.publish(message);
                shell.capture_event();
            }

            // Iced sliders otherwise react to arrow keys whenever the pointer hovers them.
            return;
        }

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );

        if self.enabled && tree.state.downcast_ref::<State>().focused {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds(),
                    border: iced::Border {
                        color: theme.extended_palette().primary.strong.color,
                        width: 2.0,
                        radius: 4.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                iced::Color::TRANSPARENT,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use iced::keyboard::key::Named;
    use iced_runtime::core::{Point, clipboard, widget::operation};

    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Pressed,
        Changed(f32),
        Input(String),
    }

    type TestElement = Element<'static, Message, Theme, ()>;

    fn test_button(enabled: bool) -> TestElement {
        button(
            widget::button(widget::Space::new().width(40).height(20)),
            enabled.then_some(Message::Pressed),
        )
    }

    fn test_slider(value: f32) -> TestElement {
        slider(0.0..=1.0, value, 0.1, Message::Changed)
    }

    fn key_press(key: Named, repeat: bool) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key),
            modified_key: keyboard::Key::Named(key),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat,
        })
    }

    struct Harness {
        element: TestElement,
        tree: Tree,
        layout: layout::Node,
    }

    impl Harness {
        fn new(mut element: TestElement) -> Self {
            let mut tree = Tree::new(element.as_widget());
            let layout = element.as_widget_mut().layout(
                &mut tree,
                &(),
                &layout::Limits::new(Size::ZERO, Size::new(800.0, 600.0)),
            );

            Self {
                element,
                tree,
                layout,
            }
        }

        fn operate(&mut self, operation: impl Operation + 'static) {
            let mut operation: Box<dyn Operation> = Box::new(operation);

            loop {
                self.element.as_widget_mut().operate(
                    &mut self.tree,
                    Layout::new(&self.layout),
                    &(),
                    operation.as_mut(),
                );

                match operation.finish() {
                    operation::Outcome::Chain(next) => operation = next,
                    _ => return,
                }
            }
        }

        fn send(&mut self, event: Event, cursor: mouse::Cursor) -> (Vec<Message>, bool) {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            self.element.as_widget_mut().update(
                &mut self.tree,
                &event,
                Layout::new(&self.layout),
                cursor,
                &(),
                &mut clipboard::Null,
                &mut shell,
                &Rectangle::with_size(Size::new(800.0, 600.0)),
            );

            let captured = shell.is_event_captured();

            (messages, captured)
        }

        fn focused(&self) -> Vec<bool> {
            fn collect(tree: &Tree, states: &mut Vec<bool>) {
                if tree.tag == tree::Tag::of::<State>() {
                    states.push(tree.state.downcast_ref::<State>().is_focused());
                } else if tree.tag == tree::Tag::of::<widget::text_input::State<()>>() {
                    states.push(
                        tree.state
                            .downcast_ref::<widget::text_input::State<()>>()
                            .is_focused(),
                    );
                }

                for child in &tree.children {
                    collect(child, states);
                }
            }

            let mut states = Vec::new();
            collect(&self.tree, &mut states);

            states
        }
    }

    #[test]
    fn button_focus_survives_rebuild_and_clears_when_disabled() {
        let mut tree = Tree::new(test_button(true).as_widget());
        tree.state.downcast_mut::<State>().focus();

        tree.diff(test_button(true).as_widget());
        assert!(tree.state.downcast_ref::<State>().is_focused());

        tree.diff(test_button(false).as_widget());
        assert!(!tree.state.downcast_ref::<State>().is_focused());

        tree.diff(test_button(true).as_widget());
        assert!(!tree.state.downcast_ref::<State>().is_focused());
    }

    #[test]
    fn slider_value_changes_preserve_focus() {
        let mut tree = Tree::new(test_slider(0.5).as_widget());
        tree.state.downcast_mut::<State>().focus();

        for value in [0.51, 1.0, 0.0, 0.42] {
            tree.diff(test_slider(value).as_widget());
            assert!(tree.state.downcast_ref::<State>().is_focused());
        }
    }

    #[test]
    fn focus_traversal_visits_inputs_buttons_and_sliders_and_skips_disabled_buttons() {
        let content = widget::Row::new()
            .push(widget::text_input("", "value").on_input(Message::Input))
            .push(test_button(true))
            .push(test_button(false))
            .push(test_slider(0.5));
        let mut harness = Harness::new(content.into());

        harness.operate(operation::focusable::focus_next());
        assert_eq!(harness.focused(), [true, false, false, false]);

        harness.operate(operation::focusable::focus_next());
        assert_eq!(harness.focused(), [false, true, false, false]);

        harness.operate(operation::focusable::focus_next());
        assert_eq!(harness.focused(), [false, false, false, true]);

        harness.operate(operation::focusable::focus_previous());
        assert_eq!(harness.focused(), [false, true, false, false]);

        harness.operate(operation::focusable::focus_previous());
        assert_eq!(harness.focused(), [true, false, false, false]);
    }

    #[test]
    fn focused_button_activates_once_and_captures_enter_and_space() {
        let mut harness = Harness::new(test_button(true));
        harness.operate(operation::focusable::focus_next());

        for key in [Named::Enter, Named::Space] {
            assert_eq!(
                harness.send(key_press(key, false), mouse::Cursor::Unavailable),
                (vec![Message::Pressed], true),
            );
            assert_eq!(
                harness.send(key_press(key, true), mouse::Cursor::Unavailable),
                (Vec::new(), false),
            );
        }

        harness.operate(operation::focusable::unfocus());

        for key in [Named::Enter, Named::Space] {
            assert_eq!(
                harness.send(key_press(key, false), mouse::Cursor::Unavailable),
                (Vec::new(), false),
            );
        }
    }

    #[test]
    fn disabled_buttons_do_not_activate_from_keyboard() {
        let mut harness = Harness::new(test_button(false));
        harness.operate(operation::focusable::focus_next());

        assert_eq!(harness.focused(), [false]);
        assert_eq!(
            harness.send(key_press(Named::Enter, false), mouse::Cursor::Unavailable),
            (Vec::new(), false),
        );
    }

    #[test]
    fn focused_slider_responds_to_arrows_and_clamps_to_its_range() {
        for (value, key, expected) in [
            (0.5, Named::ArrowLeft, 0.4),
            (0.5, Named::ArrowDown, 0.4),
            (0.5, Named::ArrowRight, 0.6),
            (0.5, Named::ArrowUp, 0.6),
            (0.95, Named::ArrowRight, 1.0),
            (0.05, Named::ArrowLeft, 0.0),
        ] {
            let mut harness = Harness::new(test_slider(value));
            harness.operate(operation::focusable::focus_next());

            assert_eq!(
                harness.send(key_press(key, false), mouse::Cursor::Unavailable),
                (vec![Message::Changed(expected)], true),
            );
        }
    }

    #[test]
    fn hovering_an_unfocused_slider_does_not_capture_arrows() {
        let mut harness = Harness::new(test_slider(0.5));
        let cursor = mouse::Cursor::Available(harness.layout.bounds().center());

        for key in [Named::ArrowUp, Named::ArrowDown] {
            assert_eq!(
                harness.send(key_press(key, false), cursor),
                (Vec::new(), false),
            );
        }
    }

    #[test]
    fn pointer_press_focuses_a_button_and_outside_press_unfocuses_it() {
        let mut harness = Harness::new(test_button(true));
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let cursor = mouse::Cursor::Available(harness.layout.bounds().center());
        let _ = harness.send(press.clone(), cursor);
        assert_eq!(harness.focused(), [true]);

        let _ = harness.send(press, mouse::Cursor::Available(Point::new(799.0, 599.0)));
        assert_eq!(harness.focused(), [false]);
    }
}
