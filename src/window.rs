use super::{
    buffer::*,
    delaying::DelayingWriter,
    Result
};

#[cfg(feature = "signals")]
use nix::{
    sys::signal,
    unistd::Pid
};

use pseudoterm::{OpenptyOptions, Winsize, WinsizeSetter};
use ransid::{Console, Event};
use std::{
    collections::VecDeque,
    ffi::OsStr,
    fs::File,
    io::prelude::*,
    mem,
    ops::{Deref, DerefMut},
    process::{Command, Child}
};

const RESIZE_LEFT: u8 = 1;
const RESIZE_RIGHT: u8 = 1 << 1;
const RESIZE_BOTTOM: u8 = 1 << 2;

pub struct Window {
    console: Console,
    inner: WindowInner
}
impl Window {
    pub fn new(cmd: &OsStr, x: u16, y: u16, width: u16, height: u16) -> Result<Self> {
        let (width, height) = (width - 2, height - 2);

        Ok(Self {
            console: Console::new(width as usize, height as usize),
            inner: WindowInner::new(cmd, x, y, width, height)?
        })
    }
    pub fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        self.inner.resize(width, height)?;
        self.console.resize(width as usize, height as usize);
        Ok(())
    }
    pub fn write(&mut self, buf: &[u8]) {
        self.inner.write(&mut self.console, buf);
    }
    fn render_frame(&self, buf: &mut Buffer, y: u16, start: char, middle: char, end: char) {
        let x = self.inner.x;

        buf.set(x, y, Char::from(start));
        buf.line(x+1, y, self.inner.width, Char::from(middle));
        buf.set(x+1+self.inner.width, y, Char::from(end));
    }
    pub fn render(&self, buf: &mut Buffer) {
        let x = self.inner.x;
        let mut y = self.inner.y;

        self.render_frame(buf, y, '┌', '─', '┐');
        y += 1;

        for row in &self.inner.screen {
            buf.set(x, y, Char::from('│'));
            buf.copy_from(x+1, y, &row);
            buf.set(x+1+self.inner.width, y, Char::from('│'));
            y += 1;
        }
        self.render_frame(buf, y, '└', '─', '┘');
    }
    pub fn inside(&self, x: u16, y: u16) -> bool {
        let start_x = self.inner.x;
        let start_y = self.inner.y;
        let end_x = self.inner.x + self.inner.width + 2;
        let end_y = self.inner.y + self.inner.height + 2;

        self.inner.drag_offset.is_some()
            || self.inner.resize != 0
            || (x >= start_x && y >= start_y && x <= end_x && y <= end_y)
    }
    pub fn click(&mut self, front: bool, m: u8, x: u16, y: u16) -> Result<()> {
        if let Some((rel_x, rel_y)) = self.inner.drag_offset {
            self.inner.x = x.saturating_sub(rel_x);
            self.inner.y = y.saturating_sub(rel_y);

            if m & 0b11 == 3 {
                self.inner.drag_offset = None;
            }
            return Ok(());
        }
        if self.inner.resize != 0 {
            let mut width = self.inner.width;
            let mut height = self.inner.height;

            if self.inner.resize & RESIZE_LEFT == RESIZE_LEFT {
                width = (self.inner.width as i32 + (self.inner.x as i32 - x as i32)) as u16;
                self.inner.x = x;
            } else if self.inner.resize & RESIZE_RIGHT == RESIZE_RIGHT {
                width = x.saturating_sub(1+self.inner.x);
            }
            if self.inner.resize & RESIZE_BOTTOM == RESIZE_BOTTOM {
                height = y.saturating_sub(1+self.inner.y);
            }

            self.resize(width, height)?;
            #[cfg(feature = "signals")]
            signal::kill(Pid::from_raw(self.inner.child.id() as _), signal::SIGWINCH)?;

            if m & 0b11 == 3 {
                self.inner.resize = 0;
            }
            return Ok(());
        }

        let x = x - self.inner.x;
        let y = y - self.inner.y;

        if y == 0 {
            self.inner.drag_offset = Some((x, y));
            return Ok(());
        }

        if x == 0 {
            self.inner.resize |= RESIZE_LEFT;
        } else if x == 1+self.inner.width {
            self.inner.resize |= RESIZE_RIGHT;
        }
        if y == 1+self.inner.height {
            self.inner.resize |= RESIZE_BOTTOM;
        }

        if self.inner.resize == 0 && front {
            // 1-based
            self.pty.write_all(&[b'\x1b', b'[', b'M', m, 32+x as u8, 32+y as u8])?;
        }
        Ok(())
    }
}
impl Deref for Window {
    type Target = WindowInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for Window {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// The whole Inner thing is a workaround because the write() function needs to
// borrow `self` which it can't do if it contains the console too.
pub struct WindowInner {
    pub pty: DelayingWriter<File>,
    pub child: Child,
    pub pty_setter: WinsizeSetter,

    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,

    pub drag_offset: Option<(u16, u16)>,
    pub resize: u8,

    pub alternate: bool,
    pub screen: VecDeque<Vec<Char>>,
    pub screen_other: VecDeque<Vec<Char>>
}
impl WindowInner {
    fn new(cmd: &OsStr, x: u16, y: u16, width: u16, height: u16) -> Result<Self> {
        // Open PTY
        let (pty, slave) = pseudoterm::openpty(&OpenptyOptions::new().with_nonblocking(true))?;
        // -> Set PTY size
        let mut pty_setter = WinsizeSetter::new(&pty)?;
        pty_setter.set(Winsize {
            cols: width,
            rows: height
        })?;
        let pty = DelayingWriter::new(pty);
        // -> Start shell
        let child = pseudoterm::prepare_cmd(slave, &mut Command::new(cmd))?.spawn()?;

        // Prepare screen buffers
        let mut screen = VecDeque::with_capacity(height as usize * 4);
        screen.resize(height as usize, vec![SPACE; width as usize]);
        let mut screen_other = VecDeque::new();
        screen_other.resize(height as usize, vec![SPACE; width as usize]);

        Ok(Self {
            pty,
            pty_setter,
            child,

            x,
            y,
            width,
            height,

            drag_offset: None,
            resize: 0,

            alternate: false,
            screen,
            screen_other
        })
    }
    fn get(&mut self, x: usize, y: usize) -> &mut Char {
        // TODO: Scrollback?
        // let screen_start = self.screen.len() - self.height as usize;

        self.screen
            .get_mut(y.min(self.height as usize - 1)).expect("invalid y in get() call")
            .get_mut(x.min(self.width as usize - 1)).expect("invalid x in get() call")
    }
    fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        self.width = width;
        self.height = height;

        for screen in &mut [&mut self.screen, &mut self.screen_other] {
            for row in &mut **screen {
                row.resize(width as usize, SPACE);
            }
            screen.resize(height as usize, vec![SPACE; width as usize]);
        }

        self.pty_setter.set(Winsize {
            cols: width,
            rows: height
        })?;
        Ok(())
    }
    fn write(&mut self, console: &mut Console, buf: &[u8]) {
        console.write(buf, |event| match event {
            Event::Char { x, y, c: content, bold, underlined, color } => {
                let c = self.get(x, y);
                c.content = content;
                c.flags = if bold { EFFECT_BOLD } else { 0 }
                    | if underlined { EFFECT_UNDERLINE } else { 0 };
                c.fg = color;
            },
            Event::Rect { x, y, w, h, color } => {
                for x in x..x+w {
                    for y in y..y+h {
                        let c = self.get(x, y);
                        c.content = ' ';
                        c.bg = color;
                    }
                }
            },
            Event::ScreenBuffer { alternate, clear } => {
                if self.alternate != alternate {
                    self.alternate = alternate;
                    mem::swap(&mut self.screen, &mut self.screen_other);
                }
                if clear {
                    // TODO: Scrollback?
                    // let scroll_len = self.screen.len() - self.height as usize;
                    // self.screen.drain(..scroll_len);

                    for row in &mut self.screen {
                        for col in row {
                            *col = SPACE;
                        }
                    }
                }
            },
            Event::Move { from_x, from_y, to_x, to_y, w, h } => {
                for rel_x in 0..w {
                    for rel_y in 0..h {
                        let rel_x = if to_x <= from_x { rel_x } else { w - rel_x };
                        let rel_y = if to_y <= from_y { rel_y } else { h - rel_y };

                        *self.get(from_x + rel_x, from_y + rel_y) = self.get(to_x + rel_x, to_y + rel_y).clone();
                    }
                }
            },
            // panics because i can't return errors here              vvvvvv
            Event::Resize { w, h } => self.resize(w as u16, h as u16).unwrap(),
            Event::Title { .. } | Event::Input { .. } => ()
        });
    }
}
