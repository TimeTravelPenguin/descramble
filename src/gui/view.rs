use std::fmt::Display;

use iced::{
    ContentFit, Element, Fill, Font, Padding, Theme,
    widget::{button, column, container, image, row, rule, scrollable, text, text_input, tooltip},
};

use crate::gui::controls_state::ValidatedBinding;

use super::app::{App, Message, Status};

pub(super) fn view(app: &App) -> Element<'_, Message> {
    let file_input = file_input_row(app);

    let status = match &app.status {
        Status::Ready => "Choose an image to shuffle and restore.".to_owned(),
        Status::Running => "Restoring the shuffled image…".to_owned(),
        Status::Complete => "Restoration complete.".to_owned(),
        Status::Failed(error) => format!("Could not complete the demo: {error}"),
    };

    let mut content = column![
        attach_label("Image path", file_input),
        algorithm_config_row(app),
        text("The demo makes a copy up to 256 pixels across before shuffling."),
        text(status),
    ]
    .spacing(16)
    .width(Fill);

    if let Some(preview) = &app.preview {
        let report = &preview.result.report;
        content = content
            .push(
                row![
                    image_card("Original", &preview.original),
                    image_card("Shuffled", &preview.scrambled),
                    image_card("Restored", &preview.restored),
                ]
                .spacing(16),
            )
            .push(text(format!(
                "Original neighbors recovered: rows {:.1}% · columns {:.1}%   |   {:.2} seconds",
                report.row_adjacency_recovery * 100.0,
                report.column_adjacency_recovery * 100.0,
                report.run.elapsed_seconds,
            )))
            .push(text("A restored image may be reflected or contain similar-looking strips in a different order."));
    }

    column![
        column![
            text("Descramble").size(32),
            text("Explore how similarities between rows and columns can restore a shuffled image."),
            rule::horizontal(2),
        ]
        .padding(Padding::default().horizontal(24).top(10))
        .spacing(4),
        scrollable(content.padding(24))
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

fn attach_label<'a>(label: &'a str, input: Element<'a, Message>) -> Element<'a, Message> {
    let label = text(label).size(14).style(text::secondary);
    column![label, input].spacing(4).into()
}

fn file_input_row(app: &App) -> Element<'_, Message> {
    let mut input = text_input("Path to a PNG or JPEG image", &app.controls.input_path).padding(10);
    let mut open = button("Browse…").padding([10, 18]);
    let mut run = button("Run demo").padding([10, 18]);

    if !app.is_running() {
        input = input.on_input(Message::InputChanged);
        open = open.on_press(Message::OpenFileDialog);

        if !app.controls.input_path.trim().is_empty() {
            run = run.on_press(Message::RunDemo);
        }
    }

    row![input, open, run].spacing(12).into()
}

fn algorithm_config_row(app: &App) -> Element<'_, Message> {
    let preview_size = int_input(
        &app.controls.preview_size,
        "Maximum height/width:",
        "Enter a number of pixels:",
        Message::PreviewSizeChanged,
    );

    let rng_seed = int_input(
        &app.controls.rng_seed,
        "Rng seed:",
        "Enter a number",
        Message::RngSeedChanged,
    );

    column![preview_size, rng_seed]
        .width(300)
        .spacing(12)
        .into()
}

fn int_input<'a, T, E>(
    input: &ValidatedBinding<T, E>,
    label: &'a str,
    placeholder: &str,
    message: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message>
where
    T: Clone + Display,
{
    let is_error = input.is_valid();

    // Do not error on empty input, but do not update the validated value either.
    let error = if input.get_raw().trim().is_empty() {
        None
    } else if !is_error {
        Some("Please enter a valid integer.")
    } else {
        None
    };

    let input = text_input(placeholder, input.get_raw()).on_input(message);

    let input = if let Some(error) = error {
        let err_input = input
            .icon(text_input::Icon {
                font: Font::DEFAULT,
                code_point: '⚠',
                size: Some(16.0.into()),
                spacing: 6.0,
                side: text_input::Side::Right,
            })
            .style(move |theme: &Theme, status| {
                let mut style = text_input::default(theme, status);
                let danger = theme.extended_palette().danger.base.color;

                style.border.color = danger;
                style.icon = danger;

                style
            });

        tooltip(
            err_input,
            container(text(error).size(14))
                .padding(8)
                .style(container::danger),
            tooltip::Position::Bottom,
        )
        .into()
    } else {
        input.into()
    };

    attach_label(label, input)
}

fn image_card<'a>(title: &'a str, handle: &image::Handle) -> Element<'a, Message> {
    container(
        column![
            text(title).size(18),
            image(handle.clone())
                .width(Fill)
                .height(256)
                .content_fit(ContentFit::Contain),
        ]
        .spacing(12),
    )
    .padding(16)
    .width(Fill)
    .style(container::rounded_box)
    .into()
}
