//! A SMTP server that can be embedded into another program
//!
//! This library provides a simple embeddable SMTP server. The
//! server uses blocking IO and a threadpool.
//! # Examples
//! ```no_run
//! use mailin_embedded::{Server, SslConfig, Handler};
//! # use mailin_embedded::err::Error;
//!
//! #[derive(Clone)]
//! struct MyHandler {}
//! impl Handler for MyHandler{}
//!
//! let handler = MyHandler {};
//! let mut server = Server::new(handler);
//!
//! server.with_name("example.com")
//!    .with_ssl(SslConfig::None)?
//!    .with_addr("127.0.0.1:25")?;
//! server.serve();
//! # Ok::<(), Error>(())
//! ```

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

/// Custom error type for mailin_embedded
pub mod err;

mod running;
mod ssl;
mod utils;

use crate::err::Error;
use crate::ssl::setup_ssl;
pub use mailin::{
    AuthMechanism, AuthResult, DataResult, Handler, HeloResult, MailResult, RcptResult,
};
use openssl::ssl::SslAcceptor;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};

/// `SslConfig` is used to configure the STARTTLS configuration of the server
pub enum SslConfig {
    /// Do not support STARTTLS
    None,
    /// Use a self-signed certificate for STARTTLS
    SelfSigned {
        /// Certificate path
        cert_path: String,
        /// Path to key file
        key_path: String,
    },
    /// Use a certificate from an authority
    Trusted {
        /// Certificate path
        cert_path: String,
        /// Key file path
        key_path: String,
        /// Path to CA bundle
        chain_path: String,
    },
}

/// `Server` is used to configure and start the SMTP server
pub struct Server<H>
where
    H: Handler + Clone + Send,
{
    handler: H,
    name: String,
    ssl_acceptor: Option<SslAcceptor>,
    num_threads: u32,
    auth: Vec<AuthMechanism>,
    tcp_listener: Option<TcpListener>,
    socket_address: Vec<SocketAddr>,
}

impl<H> Server<H>
where
    H: Handler + Clone + Send,
{
    /// Create a new server with the given Handler
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            name: "localhost".to_owned(),
            ssl_acceptor: None,
            num_threads: 4,
            auth: Vec::with_capacity(4),
            tcp_listener: None,
            socket_address: Vec::with_capacity(4),
        }
    }

    /// Give the server a name
    pub fn with_name<S>(&mut self, name: S) -> &mut Self
    where
        S: Into<String>,
    {
        self.name = name.into();
        self
    }

    /// Set the SSL configuration of the server
    pub fn with_ssl(&mut self, ssl_config: SslConfig) -> Result<&mut Self, Error> {
        self.ssl_acceptor = setup_ssl(ssl_config)?;
        Ok(self)
    }

    /// Set the size of the threadpool which is equal to the maximum number of
    /// concurrent SMTP sessions.
    pub fn with_num_threads(&mut self, num_threads: u32) -> &mut Self {
        self.num_threads = num_threads;
        self
    }

    /// Add an authentication mechanism that will supported by the server
    pub fn with_auth(&mut self, auth: AuthMechanism) -> &mut Self {
        self.auth.push(auth);
        self
    }

    /// Set a tcp listener from an already open socket
    pub fn with_tcp_listener(&mut self, listener: TcpListener) -> &mut Self {
        self.tcp_listener = Some(listener);
        self
    }

    /// Add ip addresses and ports to listen on.
    /// Returns an error if the given socket addresses are not valid.
    /// ```
    /// # use mailin_embedded::{Server, Handler};
    /// # use mailin_embedded::err::Error;
    /// # #[derive(Clone)]
    /// # struct EmptyHandler {}
    /// # impl Handler for EmptyHandler {}
    /// # let mut server = Server::new(EmptyHandler {});
    /// server.with_addr("127.0.0.1:25")?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn with_addr<A: ToSocketAddrs>(&mut self, addr: A) -> Result<&mut Self, Error> {
        for addr in addr
            .to_socket_addrs()
            .map_err(|e| Error::with_source("Invalid socket address", e))?
        {
            self.socket_address.push(addr);
        }
        Ok(self)
    }

    /// Start the SMTP server and run forever
    pub fn serve(self) -> Result<(), Error> {
        running::serve(self)
    }
}
