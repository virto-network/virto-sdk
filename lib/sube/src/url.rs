use crate::prelude::*;
use core::fmt;

/// Lightweight no_std URL parser.
/// Handles only the subset sube needs: scheme, host, port, path, query params.
#[derive(Clone, Debug)]
pub struct Url {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: Option<String>,
}

impl Url {
    pub fn parse(input: &str) -> Result<Self, ()> {
        let (scheme, rest) = input.split_once("://").ok_or(())?;
        if scheme.is_empty() {
            return Err(());
        }
        let scheme = scheme.to_lowercase();

        // Split off query string
        let (authority_path, query) = match rest.split_once('?') {
            Some((ap, q)) => (ap, Some(String::from(q))),
            None => (rest, None),
        };

        // Split authority from path
        let (authority, path) = match authority_path.find('/') {
            Some(i) => (&authority_path[..i], &authority_path[i..]),
            None => (authority_path, "/"),
        };

        // Split host from port
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => match p.parse::<u16>() {
                Ok(port) => (h, Some(port)),
                Err(_) => (authority, None),
            },
            None => (authority, None),
        };

        if host.is_empty() {
            return Err(());
        }

        Ok(Url {
            scheme,
            host: String::from(host),
            port,
            path: String::from(path),
            query,
        })
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host_str(&self) -> Option<&str> {
        Some(&self.host)
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn set_port(&mut self, port: Option<u16>) {
        self.port = port;
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn query_pairs(&self) -> QueryPairs<'_> {
        QueryPairs {
            inner: self.query.as_deref(),
        }
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme, self.host)?;
        if let Some(port) = self.port {
            write!(f, ":{}", port)?;
        }
        write!(f, "{}", self.path)?;
        if let Some(ref q) = self.query {
            write!(f, "?{}", q)?;
        }
        Ok(())
    }
}

pub struct QueryPairs<'a> {
    inner: Option<&'a str>,
}

impl<'a> Iterator for QueryPairs<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let s = self.inner?;
            let (pair, rest) = match s.split_once('&') {
                Some((p, r)) => (p, Some(r)),
                None => (s, None),
            };
            self.inner = rest;
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            return Some((k, v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wss_url() {
        let url = Url::parse("wss://kreivo.io/system/account/0x1234").unwrap();
        assert_eq!(url.scheme(), "wss");
        assert_eq!(url.host_str(), Some("kreivo.io"));
        assert_eq!(url.port(), None);
        assert_eq!(url.path(), "/system/account/0x1234");
    }

    #[test]
    fn parse_with_port() {
        let url = Url::parse("ws://localhost:9944/test").unwrap();
        assert_eq!(url.host_str(), Some("localhost"));
        assert_eq!(url.port(), Some(9944));
        assert_eq!(url.path(), "/test");
    }

    #[test]
    fn parse_with_query() {
        let url = Url::parse("wss://kreivo.io/system/account?at=1234&foo=bar").unwrap();
        let at = url.query_pairs().find(|(k, _)| *k == "at").map(|(_, v)| v);
        assert_eq!(at, Some("1234"));
    }

    #[test]
    fn parse_no_path() {
        let url = Url::parse("wss://kreivo.io").unwrap();
        assert_eq!(url.path(), "/");
    }

    #[test]
    fn display_roundtrip() {
        let url = Url::parse("wss://kreivo.io:443/path?at=5").unwrap();
        assert_eq!(url.to_string(), "wss://kreivo.io:443/path?at=5");
    }

    #[test]
    fn invalid_url() {
        assert!(Url::parse("not-a-url").is_err());
        assert!(Url::parse("://empty").is_err());
    }
}
