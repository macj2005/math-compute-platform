use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode, size,
    },
};

pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn enter() -> io::Result<Self> {
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        if let Err(error) = enable_raw_mode() {
            let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    }
}

pub(crate) fn draw(
    content: &[String],
    message: &str,
    input_cursor: Option<usize>,
    scroll: usize,
) -> io::Result<()> {
    let (width, height) = size()?;
    let mut stdout = io::stdout();
    queue!(stdout, Hide, Clear(ClearType::All))?;
    draw_header(&mut stdout, width)?;

    for (index, line) in content
        .iter()
        .skip(scroll)
        .take(height.saturating_sub(7) as usize)
        .enumerate()
    {
        draw_line(&mut stdout, 2, 4 + index as u16, width, line)?;
    }

    if height >= 3 {
        draw_rule(&mut stdout, width, height - 3)?;
        draw_line(&mut stdout, 2, height - 2, width, message)?;
        draw_line(
            &mut stdout,
            2,
            height - 1,
            width,
            "[t] tasks  [r] request  [h] help  [q] quit  [Esc] back/home",
        )?;
    }

    if let Some(input_length) = input_cursor {
        let cursor_x = (4 + input_length).min(width.saturating_sub(1) as usize) as u16;
        queue!(stdout, MoveTo(cursor_x, 7), Show)?;
    }
    stdout.flush()
}

fn draw_header(stdout: &mut io::Stdout, width: u16) -> io::Result<()> {
    queue!(
        stdout,
        MoveTo(0, 0),
        SetForegroundColor(Color::Cyan),
        SetAttribute(Attribute::Bold),
        Print(" PARALLEL INTEGRATION ENGINE "),
        ResetColor,
        SetAttribute(Attribute::Reset)
    )?;
    draw_line(stdout, 2, 1, width, "Mac Johnson | Baylor '27")?;
    draw_line(stdout, 2, 2, width, "https://mpjohnson.dev")?;
    draw_rule(stdout, width, 3)
}

fn draw_rule(stdout: &mut io::Stdout, width: u16, row: u16) -> io::Result<()> {
    queue!(
        stdout,
        MoveTo(0, row),
        SetForegroundColor(Color::DarkGrey),
        Print("─".repeat(width as usize)),
        ResetColor
    )
}

fn draw_line(
    stdout: &mut io::Stdout,
    column: u16,
    row: u16,
    width: u16,
    text: &str,
) -> io::Result<()> {
    let available = width.saturating_sub(column + 1) as usize;
    let clipped: String = text.chars().take(available).collect();
    queue!(stdout, MoveTo(column, row), Print(clipped))
}
