use super::maybe;

use std::{
    io::{self, prelude::*},
    ops::{Deref, DerefMut}
};

pub struct DelayingWriter<W: Write> {
    inner: W,
    todo: Vec<u8>
}
impl<W: Write> DelayingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner, todo: Vec::new() }
    }
    pub fn write_todo(&mut self) -> io::Result<bool> {
        let mut written = false;
        loop {
            match maybe(self.inner.write(&self.todo))? {
                None | Some(0) => break,
                Some(n) => {
                    self.todo.drain(..n);
                    written = true;
                }
            }
        }
        Ok(written)
    }
}
impl<W: Write> Write for DelayingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_todo()?;

        let mut written = 0;

        if self.todo.is_empty() {
            while written < buf.len() {
                match maybe(self.inner.write(&buf[written..]))? {
                    None | Some(0) => break,
                    Some(n) => written += n
                }
            }
        }

        if written < buf.len() {
            self.todo.extend_from_slice(&buf[written..]);
        }

        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl<W: Write> Deref for DelayingWriter<W> {
    type Target = W;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<W: Write> DerefMut for DelayingWriter<W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
