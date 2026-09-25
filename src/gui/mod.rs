//! Iced presentation layer. Shared image workflows live in [`crate::application`].

mod app;
mod comparison;
mod comparison_canvas;
mod controls_state;
mod image_export;
mod view;
mod viewer;
mod worker;

pub fn run() -> iced::Result {
    iced::application(app::App::default, app::App::update, view::view)
        .title("Descramble")
        .subscription(app::App::subscription)
        .window(iced::window::Settings {
            min_size: Some(iced::Size::new(760.0, 560.0)),
            ..iced::window::Settings::default()
        })
        .window_size((1080.0, 720.0))
        .centered()
        .run()
}
