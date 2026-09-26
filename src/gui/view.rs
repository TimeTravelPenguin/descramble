use std::fmt::Display;

use iced::{
    ContentFit, Element, Fill, Font, Padding, Theme,
    widget::{
        button, column, container, image, progress_bar, row, rule, scrollable, text, text_input,
        tooltip,
    },
};

use crate::gui::{app::ControlsMessage, controls_state::ValidatedBinding};

use super::app::{App, Message, Status};
use super::keyboard_control;
use super::viewer::ImageKind;

pub(super) fn view(app: &App) -> Element<'_, Message> {
    if let Some(viewer) = app.viewer() {
        return viewer.view().map(Message::Viewer);
    }

    let file_input = file_input_row(app);

    let status = match &app.status {
        Status::Ready => "Choose an image to shuffle and restore.".to_owned(),
        Status::Running => "Restoring the shuffled image…".to_owned(),
        Status::Complete => "Restoration complete.".to_owned(),
        Status::Failed(error) => format!("Could not complete the experiment: {error}"),
    };

    let mut content = column![
        attach_label("Image path", file_input),
        algorithm_config_row(app),
        text(format!(
            "The experiment resizes a copy to at most {} pixels per side before shuffling.",
            app.controls.preview_size.get_validated(),
        )),
        text(status),
    ]
    .spacing(16)
    .width(Fill);

    if app.is_running() || matches!(app.status, Status::Complete) {
        content = content
            .push(progress_bar(0.0..=1.0, app.progress))
            .push(text(format!(
                "Search generations completed: {:.0}%",
                app.progress * 100.0
            )))
            .push(text(
                "Rows account for the first half; columns for the second. \
                    Preparing distances and the starting population may take time.",
            ));
    }

    if let Some(preview) = &app.preview {
        let report = &preview.result.report;
        content = content
            .push(
                row![
                    image_card("Original", &preview.original, ImageKind::Original),
                    image_card("Shuffled", &preview.scrambled, ImageKind::Shuffled),
                    image_card("Restored", &preview.restored, ImageKind::Restored),
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

    if !app.is_running() {
        input = input.on_input(|input| ControlsMessage::InputChanged(input).into());
    }

    let open = keyboard_control::button(
        button("Browse…").padding([10, 18]),
        (!app.is_running()).then_some(Message::OpenFileDialog),
    );

    let can_run = !app.is_running()
        && !app.controls.input_path.trim().is_empty()
        && app.controls.preview_size.is_valid()
        && app.controls.rng_seed.is_valid();

    let run = keyboard_control::button(
        button("Run experiment").padding([10, 18]),
        can_run.then_some(Message::RunExperiment),
    );

    row![input, open, run].spacing(12).into()
}

fn algorithm_config_row(app: &App) -> Element<'_, Message> {
    let preview_size = int_input(
        &app.controls.preview_size,
        "Maximum height/width:",
        "Enter a number of pixels:",
        |m| ControlsMessage::PreviewSizeChanged(m).into(),
        !app.is_running(),
    );

    let rng_seed = int_input(
        &app.controls.rng_seed,
        "Random seed:",
        "Enter a number",
        |m| ControlsMessage::RngSeedChanged(m).into(),
        !app.is_running(),
    );

    let generations = int_input(
        &app.controls.generations,
        "Generations:",
        "Enter a number",
        |m| ControlsMessage::GenerationsChanged(m).into(),
        !app.is_running(),
    );

    let population_size = int_input(
        &app.controls.population_size,
        "Population size:",
        "Enter a number",
        |m| ControlsMessage::PopulationSizeChanged(m).into(),
        !app.is_running(),
    );

    let local_passes = int_input(
        &app.controls.local_passes,
        "Local improvement passes:",
        "Enter a number",
        |m| ControlsMessage::LocalPassesChanged(m).into(),
        !app.is_running(),
    );

    row![
        column![preview_size, rng_seed].width(300).spacing(12),
        column![generations, population_size].width(300).spacing(12),
        column![local_passes].width(300).spacing(12),
    ]
    .spacing(24)
    .into()
}

fn int_input<'a, T, E>(
    input: &ValidatedBinding<T, E>,
    label: &'a str,
    placeholder: &str,
    message: impl Fn(String) -> Message + 'a,
    enabled: bool,
) -> Element<'a, Message>
where
    T: Clone + Display,
{
    let is_valid = input.is_valid();

    // Do not error on empty input, but do not update the validated value either.
    let error = if input.get_raw().trim().is_empty() {
        None
    } else if !is_valid {
        Some("Please enter a valid integer.")
    } else {
        None
    };

    let mut input = text_input(placeholder, input.get_raw());

    if enabled {
        input = input.on_input(message);
    }

    if error.is_some() {
        input = input
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
    }

    // Keep the input at the same tree position so validation preserves focus and selection.
    let input = tooltip(
        input,
        error.map(|error| {
            container(text(error).size(14))
                .padding(8)
                .style(container::danger)
        }),
        tooltip::Position::Bottom,
    )
    .padding(5)
    .into();

    attach_label(label, input)
}

fn image_card<'a>(title: &'a str, handle: &image::Handle, kind: ImageKind) -> Element<'a, Message> {
    keyboard_control::button(
        button(
            column![
                text(title).size(18),
                image(handle.clone())
                    .width(Fill)
                    .height(256)
                    .content_fit(ContentFit::Contain),
                text("Click to enlarge and compare").size(13),
            ]
            .spacing(12),
        )
        .padding(16)
        .width(Fill)
        .style(button::secondary),
        Some(Message::OpenImage(kind)),
    )
}

#[cfg(test)]
mod tests {
    use iced_runtime::core::{
        text::Renderer as TextRenderer,
        widget::{Tree, tree::Tag},
    };

    use super::*;

    type InputState = text_input::State<<iced::Renderer as TextRenderer>::Paragraph>;

    fn input_state(tree: &mut Tree) -> Option<&mut InputState> {
        if tree.tag == Tag::of::<InputState>() {
            return Some(tree.state.downcast_mut());
        }

        tree.children.iter_mut().find_map(input_state)
    }

    #[test]
    fn validation_changes_preserve_input_focus_and_selection() {
        let mut binding = ValidatedBinding::new(12_u32, |raw| raw.parse::<u32>());
        let build_input = |binding: &ValidatedBinding<u32, std::num::ParseIntError>| {
            int_input(
                binding,
                "Number",
                "Enter a number",
                |value| ControlsMessage::PreviewSizeChanged(value).into(),
                true,
            )
        };

        let mut tree = Tree::new(build_input(&binding).as_widget());
        let state = input_state(&mut tree).expect("input state");
        state.focus();
        state.select_range(0, 1);
        let selection = state.cursor();

        for raw in ["12x", "12", "12x", "", "12", "12x", "   ", "12"] {
            let _ = binding.update(raw);
            tree.diff(build_input(&binding).as_widget());

            let state = input_state(&mut tree).expect("input state after validation");
            assert!(state.is_focused(), "focus lost for {raw:?}");
            assert_eq!(state.cursor(), selection, "selection lost for {raw:?}");
        }
    }
}
