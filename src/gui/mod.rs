//! Iced presentation layer. Shared image workflows live in [`crate::application`].

mod app;
mod view;

pub fn run() -> iced::Result {
    iced::application(app::App::default, app::App::update, view::view)
        .title("Descramble")
        .window_size((1080.0, 640.0))
        .centered()
        .run()
}
