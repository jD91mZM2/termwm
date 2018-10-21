use super::{
    buffer::{Buffer, Char},
    window::Window,
    Result
};

use linked_hash_map::LinkedHashMap;
use mio::{*, unix::EventedFd};
use std::{
    ffi::OsStr,
    io::{self, prelude::*},
    os::unix::io::AsRawFd
};

pub struct Workspace<'a> {
    pub buffer: Buffer,
    pub poll: Poll,
    pub shell: &'a OsStr,
    pub token: Token,
    pub windows: LinkedHashMap<Token, Window>
}
impl<'a> Workspace<'a> {
    pub fn new(shell: &'a OsStr, token_offset: Token, width: u16, height: u16) -> Result<Self> {
        Ok(Self {
            buffer: Buffer::new(width, height),
            poll: Poll::new()?,
            shell,
            token: token_offset,
            windows: LinkedHashMap::new()
        })
    }
    #[cfg(feature = "signals")]
    pub fn resize(&mut self, width: u16, height: u16) {
        self.buffer.resize(width, height);
        // Move all windows that would be outside of the screen
        for (_, window) in &mut self.windows {
            window.x = window.x.min(width-1);
            window.y = window.y.min(height-1);
        }
    }
    pub fn add(&mut self, window: Window) -> Result<()> {
        self.poll.register(&EventedFd(&window.pty.as_raw_fd()), self.token, Ready::readable() | Ready::writable(), PollOpt::edge())?;
        self.windows.insert(self.token, window);
        self.token = Token(usize::from(self.token) + 1);
        Ok(())
    }
    pub fn remove(&mut self, key: Token) -> Result<()> {
        if let Some(window) = self.windows.remove(&key) {
            self.poll.deregister(&EventedFd(&window.pty.as_raw_fd()))?;
        }
        Ok(())
    }
    pub fn click(&mut self, m: u8, x: u8, y: u8) -> Result<()> {
        // 1-based, but we want 0-based
        let x = (x.saturating_sub(0o40 + 1) as u16).min(self.buffer.width-1);
        let y = (y.saturating_sub(0o40 + 1) as u16).min(self.buffer.height-1);

        if let Some((&key, _)) = self.windows.iter_mut().rev().find(|(_, w)| w.inside(x, y)) {
            let front = *self.windows.back().unwrap().0 == key;

            let window = &mut self.windows[&key];
            window.click(front, m, x, y)?;

            // Move window to front if the button is released or it's being
            // dragged
            if !front && (m & 0x40 == 0x40 || m & 0b11 == 3) {
                let win = self.windows.remove(&key).unwrap();
                self.windows.insert(key, win);
            }
        } else if m & 0b11 == 3 {
            // They clicked anywhere on the screen, let's spawn a terminal
            let width = 80.min(self.buffer.width);
            let height = 32.min(self.buffer.height);
            self.add(Window::new(
                self.shell,
                self.buffer.width / 2 - width / 2,
                self.buffer.height / 2 - height / 2,
                width,
                height
            )?)?;
        }
        Ok(())
    }
    pub fn render(&mut self) {
        self.buffer.clear();

        let x = self.buffer.width / 2 - "Click anywhere!".len() as u16 / 2;
        let y = self.buffer.height / 2;
        for (i, c) in "Click anywhere!".chars().enumerate() {
            self.buffer.set(x + i as u16, y, Char::from(c));
        }

        for window in self.windows.values() {
            window.render(&mut self.buffer);
        }
    }
}
impl<'a> Write for Workspace<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.windows.iter_mut().next_back() {
            Some((_, window)) => window.pty.write(buf),
            None => Ok(buf.len())
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self.windows.iter_mut().next_back() {
            Some((_, window)) => window.pty.flush(),
            None => Ok(())
        }
    }
}
