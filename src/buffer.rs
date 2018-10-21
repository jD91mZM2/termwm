use ransid::color::Color;
use std::{mem, io::{self, prelude::*}};

pub const EFFECT_BOLD:      u8 = 1;
pub const EFFECT_UNDERLINE: u8 = 1 << 1;

pub const SPACE: Char = Char {
    content: ' ',
    flags: 0,
    bg: Color::Ansi(0),
    fg: Color::Ansi(7)
};

#[derive(Clone, Copy)]
pub struct Char {
    pub content: char,
    pub flags: u8,
    pub bg: Color,
    pub fg: Color
}
impl From<char> for Char {
    fn from(c: char) -> Self {
        Self {
            content: c,
            flags: 0,
            bg: Color::Ansi(0),
            fg: Color::Ansi(7)
        }
    }
}
impl Eq for Char {}
impl PartialEq for Char {
    fn eq(&self, other: &Char) -> bool {
        self.content == other.content
            && self.flags == other.flags
            && self.bg.as_rgb() == other.bg.as_rgb()
            && self.fg.as_rgb() == other.fg.as_rgb()
    }
}

pub struct Buffer {
    prev: (bool, Vec<Char>),
    buf: Vec<Char>,
    pub width: u16,
    pub height: u16
}
impl Buffer {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            prev: (false, vec![SPACE; width as usize * height as usize]),
            buf: vec![SPACE; width as usize * height as usize],
            width,
            height
        }
    }
    pub fn clear(&mut self) {
        for block in &mut self.buf {
            *block = SPACE;
        }
    }
    #[cfg(feature = "signals")]
    pub fn resize(&mut self, width: u16, height: u16) {
        self.prev.0 = false;
        self.prev.1.resize(width as usize * height as usize, SPACE);
        self.buf.resize(width as usize * height as usize, SPACE);
        self.clear();
        self.width = width;
        self.height = height;
    }
    pub fn translate(&self, x: u16, y: u16) -> usize {
        if x >= self.width {
            // Return an invalid index
            return self.buf.len();
        }
        if y >= self.height {
            // Return an invalid index
            return self.buf.len();
        }
        y as usize * self.width as usize + x as usize
    }
    pub fn set(&mut self, x: u16, y: u16, val: Char) {
        let i = self.translate(x, y);
        if i < self.buf.len() {
            self.buf[i] = val;
        }
    }
    pub fn line(&mut self, x: u16, y: u16, len: u16, val: Char) {
        if y >= self.height { return; }

        let start = self.translate(x, y);
        let end = self.translate(x + len-1, y).min(self.translate(self.width-1, y));
        for i in start..=end {
            self.buf[i] = val;
        }
    }
    pub fn copy_from(&mut self, x: u16, y: u16, slice: &[Char]) {
        if y >= self.height { return; }

        let start = self.translate(x, y);
        let len = slice.len().min(self.width as usize - x as usize);
        self.buf[start..start+len].copy_from_slice(&slice[..len]);
    }

    pub fn draw<W: Write>(&mut self, w: &mut W) -> io::Result<()> {
        let (valid, ref prev) = self.prev;

        let mut last_bg = None;
        let mut last_fg = None;
        let mut last_flags = None;

        'y: for y in 0..self.height {
            let start = y as usize * self.width as usize;
            let end = start + self.width as usize;
            let mut buf = &self.buf[start..end];
            let mut prev = &prev[start..end];

            let mut x = 0;
            if valid {
                while buf[0] == prev[0] {
                    x += 1;
                    buf = &buf[1..];
                    prev = &prev[1..];

                    if buf.is_empty() {
                        continue 'y;
                    }
                }
            }

            write!(w, "\x1b[{};{}H", y+1, x+1);

            for col in buf {
                if last_flags != Some(col.flags) {
                    write!(w, "\x1b[0m")?;
                    print_color(w, 48, col.bg)?;
                    print_color(w, 38, col.fg)?;
                    if col.flags & EFFECT_BOLD == EFFECT_BOLD {
                        write!(w, "\x1b[1m")?;
                    } else if col.flags & EFFECT_UNDERLINE == EFFECT_UNDERLINE {
                        write!(w, "\x1b[4m")?;
                    }
                } else {
                    if last_bg != Some(col.bg.as_rgb()) {
                        print_color(w, 48, col.bg)?;
                        last_bg = Some(col.bg.as_rgb());
                        last_flags = None;
                    }
                    if last_fg != Some(col.fg.as_rgb()) {
                        print_color(w, 38, col.fg)?;
                        last_fg = Some(col.fg.as_rgb());
                        last_flags = None;
                    }
                }
                write!(w, "{}", col.content)?;
            }
        }

        self.prev.0 = true;
        mem::swap(&mut self.prev.1, &mut self.buf);
        Ok(())
    }
}

fn print_color<W: Write>(w: &mut W, mode: u8, color: Color) -> io::Result<()> {
    match color {
        Color::Ansi(val) => write!(w, "\x1b[{};5;{}m", mode, val),
        Color::TrueColor(r, g, b) => write!(w, "\x1b[{};2;{};{};{}m", mode, r, g, b)
    }
}
