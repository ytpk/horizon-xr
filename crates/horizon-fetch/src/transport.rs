use crate::{Error, Fetch};
use std::io::{Cursor, Read};
use std::sync::OnceLock;
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
/// setting), and `Range` requests. Without a proxy hook one agent serves every download, so its
/// connections stay open and are used again: thousands of small asset files go over the few
/// connections of the download workers instead of a connection and a TLS handshake each, which
/// a phone's network (and its router) does not keep up with for long.
pub struct Ureq {
    pub user_agent: String,
    pub proxy: Option<fn(ureq::AgentBuilder, &str) -> ureq::AgentBuilder>,
    shared: OnceLock<ureq::Agent>,
}

impl Ureq {
    pub fn new(user_agent: impl Into<String>) -> Self {
        Self {
            user_agent: user_agent.into(),
            proxy: None,
            shared: OnceLock::new(),
        }
    }

    /// Requests that go through the proxy hook of the launcher.
    pub fn with_proxy(mut self, proxy: fn(ureq::AgentBuilder, &str) -> ureq::AgentBuilder) -> Self {
        self.proxy = Some(proxy);
        self
    }

    fn builder(&self) -> ureq::AgentBuilder {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            // A silent connection is a stalled one: the next attempt resumes.
            .timeout_read(Duration::from_secs(60))
            // A connection kept for each download worker, and some to spare.
            .max_idle_connections_per_host(8)
            .user_agent(&self.user_agent)
    }

    fn agent(&self, url: &str) -> ureq::Agent {
        match self.proxy {
            Some(proxy) => proxy(self.builder(), url).build(),
            None => self.shared.get_or_init(|| self.builder().build()).clone(),
        }
    }
}

impl Transport for Ureq {
    fn open(&self, url: &str, from: u64) -> Result<Body, Error> {
        let mut request = self.agent(url).get(url);
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
