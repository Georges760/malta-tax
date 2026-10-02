//! Terminal interface: the report as a scrollable dashboard.

use std::io;
use std::rc::Rc;

use blit::{Input, Key, Sense, Sides, Sizing};
use blit_tui::Ui;
use blit_tui::atom::{Border, BorderStyle, Gauge};
use blit_tui::color::Color;
use blit_tui::layout::{Align, flex};
use blit_tui::text::{HorizontalAlign, TextAttributes, TextOptions, TextWrap};
use blit_tui::widget::{Block, Text, Title, scroll_area};

use crate::estimate::Estimate;
use crate::input::Input as Accounts;
use crate::report::{Report, Row, Section, money};

const ACCENT: Color = Color::LIGHT_CYAN;
const DIM: Color = Color::DARK_GRAY;
const BORDER: Color = Color::DARK_GRAY;
const STRONG: Color = Color::WHITE;

/// Below this width the sections are stacked in one column.
const TWO_COLUMNS: f32 = 150.0;
const PANEL_WIDTH: f32 = 100.0;

pub fn run(accounts: &Accounts, year: i32) -> io::Result<()> {
    let mut app = App::new(accounts, year);
    blit_tui::run(|ui| app.show(ui))
}

struct App<'a> {
    accounts: &'a Accounts,
    /// The years with income, and the one asked for.
    years: Vec<i32>,
    year: i32,
    view: Result<(Estimate, Report), String>,
    scroll: scroll_area::State,
}

impl<'a> App<'a> {
    fn new(accounts: &'a Accounts, year: i32) -> Self {
        let mut years: Vec<i32> = accounts.income.iter().map(|e| e.month.year).collect();
        years.push(year);
        years.sort_unstable();
        years.dedup();
        Self {
            accounts,
            years,
            year,
            view: crate::evaluate(accounts, year),
            scroll: scroll_area::State::new(),
        }
    }

    /// Moves to the year before (`-1`) or after (`1`), if there is one.
    fn select(&mut self, step: isize) {
        let current = self.years.iter().position(|y| *y == self.year);
        let Some(index) = current.and_then(|i| i.checked_add_signed(step)) else {
            return;
        };
        if let Some(&year) = self.years.get(index) {
            self.year = year;
            self.view = crate::evaluate(self.accounts, year);
            self.scroll = scroll_area::State::new();
        }
    }

    fn show(&mut self, mut ui: Ui<'_>) {
        let mut scroll = None;
        match *ui.input() {
            Input::Text('q') => return ui.context().quit(),
            Input::Text('h') => self.select(-1),
            Input::Text('l') => self.select(1),
            Input::Text('k') => scroll = Some(Scroll::Lines(-2.0)),
            Input::Text('j') => scroll = Some(Scroll::Lines(2.0)),
            Input::Key(key) if key.pressed => match key.key {
                Key::Escape => return ui.context().quit(),
                Key::ArrowLeft => self.select(-1),
                Key::ArrowRight => self.select(1),
                Key::ArrowUp => scroll = Some(Scroll::Lines(-2.0)),
                Key::ArrowDown => scroll = Some(Scroll::Lines(2.0)),
                Key::PageUp => scroll = Some(Scroll::Pages(-1.0)),
                Key::PageDown => scroll = Some(Scroll::Pages(1.0)),
                Key::Home => scroll = Some(Scroll::Top),
                Key::End => scroll = Some(Scroll::Bottom),
                _ => {}
            },
            _ => {}
        }

        let wide = ui.screen().size().width >= TWO_COLUMNS;
        let first = self.years.first() == Some(&self.year);
        let last = self.years.last() == Some(&self.year);
        let mut root = ui.layout(flex::column().padding(Sides::x(1.0)).gap(1.0));

        let step = root
            .child()
            .item(flex::item().height(Sizing::fixed(1.0)))
            .build(|ui: Ui<'_>| header(ui, &self.accounts.taxpayer.name, self.year, first, last));

        match &self.view {
            Ok((estimate, report)) => {
                root.child()
                    .item(flex::item().height(Sizing::fixed(4.0)))
                    .build(|ui: Ui<'_>| cards(ui, self.accounts, estimate));
                root.child()
                    .item(flex::item().height(Sizing::fixed(1.0)))
                    .build(|ui: Ui<'_>| progress(ui, estimate));
                root.child()
                    .item(flex::item().grow())
                    .build(scroll_area::new(
                        &mut self.scroll,
                        scroll_area::Config::new(),
                        |ui: Ui<'_>| sections(ui, &report.sections, wide),
                        |active| {
                            let thumb = if active { ACCENT } else { DIM };
                            (
                                Some(Block::new().background(Color::BLACK)),
                                Some(Block::new().background(thumb)),
                            )
                        },
                    ));
                // The scroll area has just measured itself: only now is the
                // extent of a page, or of the whole content, known.
                if let Some(scroll) = scroll {
                    let state = &mut self.scroll;
                    match scroll {
                        Scroll::Lines(lines) => state.scroll_by(lines),
                        Scroll::Pages(pages) => state.scroll_by(pages * state.viewport_extent),
                        Scroll::Top => state.scroll_to(0.0),
                        Scroll::Bottom => state.scroll_to(state.maximum_offset()),
                    }
                    root.request_frame();
                }
            }
            Err(error) => {
                root.child().item(flex::item().grow()).build(|ui: Ui<'_>| {
                    let mut panel = ui.layout(flex::column().padding(Sides::xy(2.0, 1.0)));
                    panel.insert(frame(" NO ESTIMATE "));
                    panel.child().insert(Text::new(error).options(wrapped()));
                });
            }
        }

        root.child()
            .item(flex::item().height(Sizing::fixed(1.0)))
            .insert(Text::new("←/→ year   ↑/↓ PgUp/PgDn scroll   q quit").color(DIM));

        if step != 0 {
            self.select(step);
            root.request_frame();
        }
    }
}

/// A move of the report asked for with the keyboard.
enum Scroll {
    Lines(f32),
    Pages(f32),
    Top,
    Bottom,
}

/// The title bar. Returns the step asked for with the year arrows.
fn header(ui: Ui<'_>, name: &str, year: i32, first: bool, last: bool) -> isize {
    let mut row = ui.layout(flex::row().gap(1.0).align(Align::Center));
    row.child().build(|ui: Ui<'_>| {
        let mut badge = ui.layout(flex::row());
        badge.insert(Block::new().background(ACCENT));
        badge.child().insert(
            Text::new(" malta-tax ")
                .color(Color::BLACK)
                .attributes(TextAttributes::BOLD),
        );
    });
    row.child()
        .insert(Text::new(name).attributes(TextAttributes::BOLD));
    row.child().item(flex::item().grow()).insert(());
    let previous = row.child().build(arrow("◀", !first));
    row.child().insert(
        Text::new(&year.to_string())
            .color(ACCENT)
            .attributes(TextAttributes::BOLD),
    );
    let next = row.child().build(arrow("▶", !last));
    isize::from(next) - isize::from(previous)
}

/// A clickable arrow. Returns whether it was clicked.
fn arrow(label: &str, enabled: bool) -> impl FnOnce(Ui<'_>) -> bool {
    move |mut ui: Ui<'_>| {
        let interaction = ui.interact(Sense::CLICK);
        let color = match (enabled, interaction.hovered) {
            (false, _) => DIM,
            (true, true) => STRONG,
            (true, false) => ACCENT,
        };
        ui.insert(Text::new(label).color(color));
        enabled && interaction.clicked
    }
}

/// The four figures that matter, side by side. The texts are short enough
/// for a terminal of 80 columns.
fn cards(ui: Ui<'_>, accounts: &Accounts, estimate: &Estimate) {
    let ssc = &estimate.social_security;
    let received = estimate.months.iter().filter(|m| !m.projected).count();
    let cards = [
        (
            " GROSS INCOME ",
            format!("{} EUR", money(estimate.gross_income)),
            format!("{received} of {} months", estimate.months.len()),
        ),
        (
            " INCOME TAX ",
            format!("{} EUR", money(estimate.income_tax)),
            format!("{} rates", accounts.taxpayer.tax_status),
        ),
        (
            " SOCIAL SEC. ",
            format!("{} EUR", money(ssc.total)),
            format!("{} wk x {}", ssc.weeks, money(ssc.weekly)),
        ),
        (
            " SET ASIDE ",
            format!("{:.1} %", estimate.set_aside_rate * 100.0),
            "of each payment".to_owned(),
        ),
    ];
    let mut row = ui.layout(flex::row().gap(1.0));
    for (title, value, note) in &cards {
        row.child().item(flex::item().grow()).build(|ui: Ui<'_>| {
            let mut card = ui.layout(flex::column().padding(Sides::xy(2.0, 1.0)));
            card.insert(frame(title));
            card.child().insert(
                Text::new(value)
                    .color(STRONG)
                    .attributes(TextAttributes::BOLD),
            );
            card.child().insert(Text::new(note).color(DIM));
        });
    }
}

/// How much of the year's income is in, and how much of the total is paid.
fn progress(ui: Ui<'_>, estimate: &Estimate) {
    let ratio = |part: f64, whole: f64| if whole > 0.0 { part / whole } else { 0.0 };
    let gauges = [
        (
            ratio(estimate.income_received, estimate.gross_income),
            format!(
                "received {} of {} EUR",
                money(estimate.income_received),
                money(estimate.gross_income)
            ),
            Color::GREEN,
        ),
        (
            ratio(estimate.paid(), estimate.total),
            format!(
                "paid {} of {} EUR",
                money(estimate.paid()),
                money(estimate.total)
            ),
            Color::BLUE,
        ),
    ];
    let mut row = ui.layout(flex::row().gap(1.0));
    for (ratio, label, color) in gauges {
        row.child().item(flex::item().grow()).insert(
            Gauge::new(ratio)
                .label(Rc::from(label))
                .filled(color)
                .unfilled(Color::BLACK)
                .label_color(STRONG),
        );
    }
}

/// Every section of the report as a panel, in one or two columns.
fn sections(ui: Ui<'_>, sections: &[Section], wide: bool) {
    if !wide {
        return column(ui, sections);
    }
    // The income table and the deductions on the left, the taxes on the right.
    let (left, right) = sections.split_at(sections.len().min(2));
    let mut row = ui.layout(flex::row().gap(1.0));
    for half in [left, right] {
        row.child()
            .item(flex::item().width(Sizing::grow()))
            .build(|ui: Ui<'_>| column(ui, half));
    }
}

fn column(ui: Ui<'_>, sections: &[Section]) {
    let mut column = ui.layout(flex::column().gap(1.0).align(Align::Start));
    for section in sections {
        // Wider than this, an amount is too far from its label to be read.
        column
            .child()
            .item(flex::item().width(Sizing::grow_range(0.0, PANEL_WIDTH)))
            .build(|ui: Ui<'_>| panel(ui, section));
    }
}

fn panel(ui: Ui<'_>, section: &Section) {
    let remarks = section.rows.iter().any(|row| {
        matches!(
            row,
            Row::Amount {
                remark: Some(_),
                ..
            }
        )
    });
    let mut panel = ui.layout(flex::column().padding(Sides::xy(2.0, 1.0)));
    panel.insert(frame(&format!(" {} ", section.title.to_uppercase())));
    for row in &section.rows {
        match row {
            Row::MonthHeader => {
                let cells = ["month", "USD", "EUR", "EUR/USD", ""];
                panel
                    .child()
                    .build(|ui: Ui<'_>| month(ui, cells, DIM, TextAttributes::NONE));
            }
            Row::Month {
                month: name,
                usd,
                eur,
                rate,
                remark,
            } => {
                let cells = [name, usd, eur, rate, remark].map(String::as_str);
                let color = if remark.starts_with("projected") {
                    DIM
                } else {
                    Color::Reset
                };
                panel
                    .child()
                    .build(|ui: Ui<'_>| month(ui, cells, color, TextAttributes::NONE));
            }
            Row::Amount {
                label,
                value,
                indent,
                strong,
                remark,
            } => {
                panel.child().build(|ui: Ui<'_>| {
                    let (color, attributes) = match (*strong, *indent) {
                        (true, _) => (STRONG, TextAttributes::BOLD),
                        (false, true) => (DIM, TextAttributes::NONE),
                        (false, false) => (Color::Reset, TextAttributes::NONE),
                    };
                    let label = format!("{}{label}", if *indent { "  " } else { "" });
                    let mut line = ui.layout(flex::row());
                    line.child()
                        .item(flex::item().width(Sizing::grow()))
                        .insert(Text::new(&label).color(color).attributes(attributes));
                    line.child()
                        .item(flex::item().width(Sizing::fixed(14.0)))
                        .insert(
                            Text::new(&money(*value))
                                .color(color)
                                .attributes(attributes)
                                .options(right()),
                        );
                    if remarks {
                        let remark = remark.as_deref().unwrap_or_default();
                        line.child()
                            .item(flex::item().width(Sizing::fixed(12.0)))
                            .insert(Text::new(remark).color(DIM).options(right()));
                    }
                });
            }
            Row::Text { text, indent } => {
                // Wrapping drops leading spaces, so the indent is a cell.
                let margin = if *indent { 2.0 } else { 0.0 };
                let mut line = panel.child().layout(flex::row());
                line.child()
                    .item(flex::item().width(Sizing::fixed(margin)))
                    .insert(());
                line.child()
                    .item(flex::item().width(Sizing::grow()))
                    .insert(Text::new(text).color(DIM).options(wrapped()));
            }
            Row::Gap => {
                panel
                    .child()
                    .item(flex::item().height(Sizing::fixed(1.0)))
                    .insert(());
            }
        }
    }
}

/// One line of the monthly income table.
fn month(ui: Ui<'_>, cells: [&str; 5], color: Color, attributes: TextAttributes) {
    let [name, usd, eur, rate, remark] = cells;
    let mut line = ui.layout(flex::row());
    line.child()
        .item(flex::item().width(Sizing::fixed(9.0)))
        .insert(Text::new(name).color(color).attributes(attributes));
    for (cell, width) in [(usd, 12.0), (eur, 12.0), (rate, 10.0)] {
        line.child()
            .item(flex::item().width(Sizing::fixed(width)))
            .insert(Text::new(cell).color(color).options(right()));
    }
    line.child()
        .item(flex::item().width(Sizing::grow()))
        .insert(Text::new(&format!("  {remark}")).color(DIM));
}

/// The rounded border and title of a panel.
fn frame(title: &str) -> Block<'_> {
    Block::new()
        .border(Border::new(BORDER).style(BorderStyle::Rounded))
        .title(
            Title::new(title)
                .color(ACCENT)
                .attributes(TextAttributes::BOLD),
        )
}

fn right() -> TextOptions {
    TextOptions::new().horizontal_align(HorizontalAlign::Right)
}

fn wrapped() -> TextOptions {
    TextOptions::new().wrap(TextWrap::Word)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use blit::{Frame, FrameInfo, KeyInput, LayoutResolution, Size};
    use blit_tui::{RendererConfig, TuiContext, TuiRenderer};

    use super::*;

    /// A terminal of a given size, without the terminal: frames are painted
    /// into the renderer's cells and read back as text.
    struct Screen {
        context: TuiContext,
        frame: Frame<TuiContext>,
        info: FrameInfo,
    }

    impl Screen {
        fn new(columns: u16, rows: u16) -> Self {
            let config = RendererConfig::new().columns(columns).rows(rows);
            let size = Size::new(f32::from(columns), f32::from(rows));
            Self {
                context: TuiContext::new(TuiRenderer::new(config)),
                frame: Frame::default(),
                info: FrameInfo::new(size).layout_resolution(LayoutResolution::Discrete {
                    step: Size::uniform(1.0),
                }),
            }
        }

        fn show(&mut self, app: &mut App<'_>, input: Input) -> String {
            let time = Duration::ZERO;
            self.frame
                .build(&mut self.context, self.info, time, input, |ui: Ui<'_>| {
                    app.show(ui)
                });
            self.frame.layout(&mut self.context);
            self.context.begin_paint();
            self.frame.paint(&mut self.context);
            self.context.finish_paint();
            self.context.renderer().plain_text()
        }
    }

    fn example() -> Accounts {
        serde_json::from_str(include_str!("../example.json")).expect("valid example")
    }

    #[test]
    fn shows_the_report() {
        let accounts = example();
        let mut app = App::new(&accounts, 2026);
        let text = Screen::new(120, 60).show(&mut app, Input::None);
        println!("{text}");
        for expected in [
            "malta-tax",
            "Maria Borg",
            "GROSS INCOME",
            "86,445.70 EUR",
            "25.6 %",
            "of each payment",
            "received 60,519.40 of 86,445.70 EUR",
            "INCOME",
            "2026-09",
            "projected",
            "gross income",
            "DEDUCTIONS",
        ] {
            assert!(text.contains(expected), "{expected:?} is not on screen");
        }
    }

    #[test]
    fn wide_screen_has_two_columns() {
        let accounts = example();
        let mut app = App::new(&accounts, 2026);
        let text = Screen::new(170, 50).show(&mut app, Input::None);
        println!("{text}");
        let line = |needle| text.lines().position(|l: &str| l.contains(needle));
        // The income table and the income tax start on the same line.
        assert!(line(" INCOME ").is_some());
        assert_eq!(line(" INCOME "), line(" INCOME TAX "));
    }

    #[test]
    fn a_year_without_rules_says_so() {
        let accounts = example();
        let mut app = App::new(&accounts, 2031);
        assert_eq!(app.years, [2026, 2031]);
        let mut screen = Screen::new(100, 20);
        let text = screen.show(&mut app, Input::None);
        assert!(text.contains("no tax rules for 2031"), "{text}");

        // The left arrow goes back to the year that has an estimate.
        let text = screen.show(&mut app, Input::Key(KeyInput::new(Key::ArrowLeft)));
        assert_eq!(app.year, 2026);
        assert!(text.contains("GROSS INCOME"), "{text}");
    }

    #[test]
    fn scrolls_to_the_end() {
        let accounts = example();
        let mut app = App::new(&accounts, 2026);
        let mut screen = Screen::new(80, 24);
        let text = screen.show(&mut app, Input::None);
        println!("{text}");
        assert!(!text.contains("COMING UP"));

        // The first frame measured the content; End then goes to its bottom.
        screen.show(&mut app, Input::Key(KeyInput::new(Key::End)));
        let text = screen.show(&mut app, Input::None);
        println!("{text}");
        assert!(text.contains("COMING UP"), "{text}");
        assert!(text.contains("provisional tax"), "{text}");
    }
}
