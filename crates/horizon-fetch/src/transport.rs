use crate::{Error, Fetch};
use std::io::{Cursor, Read};
use std::time::Duration;

/// An open response body.
pub struct Body {
    pub reader: Box<dyn Read + Send>,
    /// Whether the server honoured the `Range`: the bytes start at the offset asked for, not at
    /// the file's start.
    pub partial: bool,
}

/// Where bytes come from.
pub trait Transport: Sync {
    /// Opens `url`, from byte `from` on when `from > 0` (the answer says if it is honoured).
    /// A refusal is [`Error::Status`]; a connection that fails is [`Error::Network`].
    fn open(&self, url: &str, from: u64) -> Result<Body, Error>;
}

/// Real downloads: `ureq` with rustls, a user agent, an optional proxy hook (the launcher's
/// setting), and `Range` requests.
pub struct Ureq {
    pub user_agent: String,
    pub proxy: Option<fn(ureq::AgentBuilder, &str) -> ureq::AgentBuilder>,
}

impl Ureq {
    pub fn new(user_agent: impl Into<String>) -> Self {
        Self {
            user_agent: user_agent.into(),
            proxy: None,
        }
    }
}

impl Transport for Ureq {
    fn open(&self, url: &str, from: u64) -> Result<Body, Error> {
        let builder = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            // A silent connection is a stalled one: the next attempt resumes.
            .timeout_read(Duration::from_secs(60))
            .user_agent(&self.user_agent);
        let builder = match self.proxy {
            Some(proxy) => proxy(builder, url),
            None => builder,
        };
        let mut request = builder.build().get(url);
        if from > 0 {
            request = request.set("Range", &format!("bytes={from}-"));
        }
        let network = |message: String| Error::Network {
            url: url.into(),
            message,
        };
        match request.call() {
            Ok(response) => Ok(Body {
                partial: response.status() == 206,
                reader: response.into_reader(),
            }),
            Err(ureq::Error::Status(status, _)) => Err(Error::Status {
                url: url.into(),
                status,
            }),
            // Never the URL's query: a transport error names the host only.
            Err(ureq::Error::Transport(t)) => Err(network(format!(
                "could not be reached ({:?}). Check your internet connection.",
                t.kind()
            ))),
        }
    }
}

/// A whole-body function as a transport: it has no ranges, so the body is cut at the offset.
pub struct Bodies<'a>(pub Fetch<'a>);

impl Transport for Bodies<'_> {
    fn open(&self, url: &str, from: u64) -> Result<Body, Error> {
        let body = (self.0)(url).map_err(|message| Error::Network {
            url: url.into(),
            message,
        })?;
        let from = (from as usize).min(body.len());
        let mut cursor = Cursor::new(body);
        cursor.set_position(from as u64);
        Ok(Body {
            reader: Box::new(cursor),
            partial: from > 0,
        })
    }
}
