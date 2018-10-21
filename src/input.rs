use super::Result;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    ESC,
    CSI,
    Mouse
}
impl Default for State {
    fn default() -> Self {
        State::Normal
    }
}

#[derive(Debug)]
pub enum Event {
    Unsupported(Vec<u8>),
    Mouse(u8, u8, u8)
}

#[derive(Default)]
pub struct Parser {
    state: State,
    arg1: Option<u8>,
    arg2: Option<u8>,
    arg3: Option<u8>
}
impl Parser {
    pub fn new() -> Self {
        Self::default()
    }
    /// Returns true if the byte should be sent like normal
    pub fn feed<F>(&mut self, byte: u8, mut performer: F) -> Result<bool>
        where F: FnMut(Event) -> Result<()>
    {
        let was_normal = self.state == State::Normal;
        match self.state {
            State::Normal => if byte == b'\x1b' {
                self.state = State::ESC;
            },
            State::ESC => if byte == b'[' {
                self.state = State::CSI;
            } else {
                self.state = State::Normal;
                performer(Event::Unsupported(vec![b'\x1b', byte]))?;
            },
            State::CSI => if byte == b'M' {
                self.state = State::Mouse;
            } else {
                self.state = State::Normal;
                performer(Event::Unsupported(vec![b'\x1b', b'[', byte]))?;
            },
            State::Mouse => if self.arg1.is_none() {
                self.arg1 = Some(byte);
            } else if self.arg2.is_none() {
                self.arg2 = Some(byte);
            } else {
                self.arg3 = Some(byte);

                performer(Event::Mouse(
                    self.arg1.unwrap(),
                    self.arg2.unwrap(),
                    self.arg3.unwrap()
                ))?;
                self.arg1 = None;
                self.arg2 = None;
                self.arg3 = None;
                self.state = State::Normal;
            }
        }
        Ok(was_normal && self.state == State::Normal)
    }
}
