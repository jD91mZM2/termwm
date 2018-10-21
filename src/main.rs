#[cfg(feature = "failure")] use failure::Error;
#[cfg(feature = "signals")] use mio::unix::EventedFd;
#[cfg(feature = "signals")]
use nix::sys::{
    signal::{self, SigSet},
    signalfd::{SignalFd, SfdFlags}
};
#[cfg(feature = "signals")]
use std::os::unix::io::AsRawFd;

use mio::*;
use pseudoterm::RawTerminal;
use std::{
    borrow::Cow,
    env,
    ffi::OsStr,
    io::{self, prelude::*},
    ops::{Deref, DerefMut},
    time::{Duration, Instant}
};

mod buffer;
mod delaying;
mod input;
mod stdin;
mod window;
mod workspace;

use self::input::{Parser, Event};
use self::stdin::MioStdin;
use self::workspace::Workspace;

const REDRAW_TIMER: u64 = 1_000;

#[cfg(feature = "signals")]
const TOKEN_SIGNAL: Token = Token(0);
const TOKEN_STDIN: Token = Token(1);
const TOKEN_PTY: Token = Token(2);

pub fn maybe<T>(err: io::Result<T>) -> io::Result<Option<T>> {
    match err {
        Ok(inner) => Ok(Some(inner)),
        Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(err) => Err(err)
    }
}

struct Restorer<W, D>(D)
    where W: Write,
          D: Deref<Target = W> + DerefMut;
impl<W, D> Deref for Restorer<W, D>
    where W: Write,
          D: Deref<Target = W> + DerefMut
{
    type Target = D;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<W, D> DerefMut for Restorer<W, D>
    where W: Write,
          D: Deref<Target = W> + DerefMut
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<W, D> Drop for Restorer<W, D>
    where W: Write,
          D: Deref<Target = W> + DerefMut
{
    fn drop(&mut self) {
        let _ = write!(self.0, "\x1b[?1002l\x1b[?1000l\x1b[?25h\x1b[?1049l");
    }
}

#[cfg(not(feature = "failure"))]
pub type Result<T> = io::Result<T>;
#[cfg(feature = "failure")]
pub type Result<T> = std::result::Result<T, Error>;

fn main() -> Result<()> {
    let shell: Cow<OsStr> = env::args_os().skip(1)
        .next()
        .or_else(|| env::var_os("SHELL"))
        .map(Cow::Owned)
        .unwrap_or(Cow::Borrowed(OsStr::new("bash")));

    // Temporary: Use stderr because stdout is being filled with "Unknown CSI:"
    let stdout = io::stderr();
    let stdin = MioStdin::new();

    let size = pseudoterm::get_size(&stdout)?;
    //let mut stdout = stdout.lock();
    let mut stdout = RawTerminal::new(stdout)?;

    let mut workspace = Workspace::new(&shell, TOKEN_PTY, size.cols, size.rows)?;
    workspace.poll.register(&stdin.reg, TOKEN_STDIN, Ready::readable(), PollOpt::edge())?;

    #[cfg(feature = "signals")]
    let mut signalfd = {
        let mut mask = SigSet::empty();
        mask.add(signal::SIGWINCH); // Window resize event
        mask.thread_block()?;

        let signalfd = SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK)?;
        workspace.poll.register(&EventedFd(&signalfd.as_raw_fd()), TOKEN_SIGNAL, Ready::readable(), PollOpt::edge())?;
        signalfd
    };

    write!(stdout, "\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1002h")?;
    let mut stdout = Restorer(stdout);

    let mut parser = Parser::new();

    workspace.render();

    workspace.buffer.draw(&mut **stdout)?;
    stdout.flush()?;

    let mut buf = [0; 1024];
    let mut events = Events::with_capacity(1024);
    let mut timeout = None;
    let mut last = Instant::now();
    'main: loop {
        workspace.poll.poll(&mut events, timeout)?;

        for event in events.iter() {
            match event.token() {
                #[cfg(feature = "signals")]
                TOKEN_SIGNAL => while let Some(_) = signalfd.read_signal()? {
                    let size = pseudoterm::get_size(&**stdout)?;
                    workspace.resize(size.cols, size.rows);
                },
                TOKEN_STDIN => {
                    while let Ok(buf) = stdin.rx.try_recv() {
                        // Only write bytes where parser's `feed` returned true
                        let mut start = None;
                        for (i, &b) in buf.iter().enumerate() {
                            let pass = parser.feed(b, |event| match event {
                                Event::Unsupported(vec) => workspace.write_all(&vec).map_err(|err| err.into()),
                                Event::Mouse(m, x, y) => workspace.click(m, x, y)
                            })?;

                            if pass {
                                start = Some(i);
                            } else if let Some(s) = start {
                                workspace.write_all(&buf[s..i])?;
                                start = None;
                            }
                        }
                        if let Some(s) = start {
                            workspace.write_all(&buf[s..])?;
                        }
                    }
                    workspace.flush()?;
                },
                token if token >= TOKEN_PTY => if let Some(window) = workspace.windows.get_mut(&token) {
                    if event.readiness().is_writable()
                            && window.pty.write_todo()? {
                        window.pty.flush()?;
                    }
                    if event.readiness().is_readable() {
                        loop {
                            match maybe(window.pty.read(&mut buf)) {
                                Ok(Some(0)) | Err(_) => {
                                    window.child.wait()?;

                                    workspace.remove(token)?;
                                    if workspace.windows.is_empty() {
                                        break 'main;
                                    } else {
                                        break;
                                    }
                                },
                                Ok(None) => break,
                                Ok(Some(n)) => window.write(&buf[..n])
                            }
                        }
                    }
                },
                _ => ()
            }
        }

        let elapsed = last.elapsed();
        if elapsed >= Duration::from_nanos(REDRAW_TIMER) {
            workspace.render();

            workspace.buffer.draw(&mut **stdout)?;
            stdout.flush()?;

            timeout = None;
            last = Instant::now();
        } else {
            timeout = Some(Duration::from_nanos(REDRAW_TIMER) - elapsed);
        }
    }
    Ok(())
}
