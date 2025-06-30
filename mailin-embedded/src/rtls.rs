use crate::ssl::SslConfig;
use crate::{Error, Stream};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{Error as TLSError, ServerConfig, ServerConnection, StreamOwned};
use std::fs;
use std::io::BufReader;
use std::sync::Arc;

// Rustls wrapper
#[derive(Clone)]
pub struct SslImpl {
    tls_config: Arc<ServerConfig>,
}

impl<S: Stream> Stream for StreamOwned<ServerConnection, S> {}

impl From<TLSError> for Error {
    fn from(error: TLSError) -> Self {
        let msg = error.to_string();
        Error::with_source(msg, error)
    }
}

impl SslImpl {
    pub fn setup(ssl_config: SslConfig) -> Result<Option<Self>, Error> {
        let config = match ssl_config {
            SslConfig::Trusted {
                cert_path,
                key_path,
                chain_path,
            } => {
                let mut certs = load_certs(&cert_path)?;
                let mut chain = load_certs(&chain_path)?;
                certs.append(&mut chain);
                let key = load_key(&key_path)?;
                let config = ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(certs, key)?;
                Some(config)
            }
            SslConfig::SelfSigned {
                cert_path,
                key_path,
            } => {
                let certs = load_certs(&cert_path)?;
                let key = load_key(&key_path)?;
                let config = ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(certs, key)?;
                Some(config)
            }
            _ => None,
        };
        let ret = config.map(|c| SslImpl {
            tls_config: Arc::new(c),
        });
        Ok(ret)
    }

    pub fn accept<S: Stream>(&self, stream: S) -> Result<impl Stream, Error> {
        let session = ServerConnection::new(self.tls_config.clone())?;
        let tls_stream = StreamOwned::new(session, stream);
        Ok(tls_stream)
    }
}

fn load_certs(filename: &str) -> Result<Vec<CertificateDer<'static>>, Error> {
    let certfile = fs::File::open(filename)?;
    let mut reader = BufReader::new(certfile);
    let ret: Result<Vec<_>, _> = rustls_pemfile::certs(&mut reader).collect();
    ret.map_err(|_| Error::new("Unparseable certificates"))
}

fn load_key(filename: &str) -> Result<PrivateKeyDer<'static>, Error> {
    // Prefer to load pkcs8 keys
    let keyfile = fs::File::open(filename)?;
    let mut reader = BufReader::new(keyfile);
    let Some(key) = rustls_pemfile::private_key(&mut reader)
        .map_err(|_| Error::new("Unparseable PKCS8 key"))?
    else {
        return Err(Error::new("No private certificate keys found in pem"));
    };
    Ok(key)
}
