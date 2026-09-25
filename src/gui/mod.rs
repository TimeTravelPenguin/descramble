//! Iced presentation layer. Shared image workflows live in [`crate::application`].

mod app;
mod controls_state;
mod view;
mod worker;

pub fn run() -> iced::Result {
    iced::application(app::App::default, app::App::update, view::view)
        .title("Descramble")
        .window_size((1080.0, 720.0))
        .centered()
        .run()
}
