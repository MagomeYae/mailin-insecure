use std::fmt::Debug;
use std::io::{stdin, stdout, Read, StdinLock, StdoutLock, Write};
use std::net::TcpStream;

/// The stream of a connection
pub trait Stream: Read + Write + Debug + 'static {}

impl Stream for TcpStream {}

/// Stdio as a [`Stream`]
#[derive(Debug)]
pub struct Stdio {
    r: StdinLock<'static>,
    w: StdoutLock<'static>,
}

impl Read for Stdio {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.r.read(buf)
    }
}
impl Write for Stdio {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.w.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

impl Stream for Stdio {}

impl Stdio {
    /// Create a `Stdio` by locking the standard input and output streams
    ///
    /// See [`Stdin::lock`](std::io::Stdin::lock) and [`Stdout::lock`](std::io::Stdout::lock).
    pub fn lock() -> Self {
        Stdio {
            r: stdin().lock(),
            w: stdout().lock(),
        }
    }
}
