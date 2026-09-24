use iced::{
    ContentFit, Element, Fill,
    widget::{button, column, container, image, row, scrollable, text, text_input},
};

use super::app::{App, Message, Status};

pub(super) fn view(app: &App) -> Element<'_, Message> {
    let mut input = text_input("Path to a PNG or JPEG image", &app.input_path).padding(10);
    let mut run = button("Run demo").padding([10, 18]);

    if !app.is_running() {
        input = input.on_input(Message::InputChanged);

        if !app.input_path.trim().is_empty() {
            run = run.on_press(Message::RunDemo);
        }
    }

    let status = match &app.status {
        Status::Ready => "Choose an image to shuffle and restore.".to_owned(),
        Status::Running => "Restoring the shuffled image…".to_owned(),
        Status::Complete => "Restoration complete.".to_owned(),
        Status::Failed(error) => format!("Could not complete the demo: {error}"),
    };

    let mut content = column![
        text("Descramble").size(32),
        text("Explore how similarities between rows and columns can restore a shuffled image."),
        text("Image path").size(14),
        row![input, run].spacing(12),
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

    container(scrollable(content))
        .padding(24)
        .width(Fill)
        .height(Fill)
        .into()
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
