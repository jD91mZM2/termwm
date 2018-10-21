use mio::{Ready, Registration};
use std::{
    io::{self, prelude::*},
    sync::mpsc,
    thread
};

pub struct MioStdin {
    pub reg: Registration,
    pub rx: mpsc::Receiver<Vec<u8>>
}
impl MioStdin {
    pub fn new() -> Self {
        let (reg, setter) = Registration::new2();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let stdin = io::stdin();
            let mut stdin = stdin.lock();

            let mut buf = [0; 1024];
            loop {
                match stdin.read(&mut buf).unwrap() {
                    0 => break,
                    n => {
                        tx.send(buf[..n].to_vec()).unwrap();
                        setter.set_readiness(Ready::readable()).unwrap();
                    }
                }
            }
        });

        Self { reg, rx }
    }
}
