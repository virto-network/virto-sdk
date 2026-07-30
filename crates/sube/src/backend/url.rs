use crate::prelude::*;
use core::fmt;

/// Lightweight no_std URL parser.
/// Handles only the subset sube needs: scheme, host, port, path, query params.
#[derive(Clone, Debug)]
pub struct Url {
    scheme: String,
    host: String,
    bracketed_host: bool,
    port: Option<u16>,
    path: String,
    query: Option<String>,
}

impl Url {
    pub fn parse(input: &str) -> Result<Self, ()> {
        if input.is_empty()
            || input.bytes().any(|byte| byte.is_ascii_whitespace())
            || input.contains('#')
        {
            return Err(());
        }
        let (scheme, rest) = input.split_once("://").ok_or(())?;
        if !matches!(scheme, "ws" | "wss" | "http" | "https") {
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

        if authority.is_empty() || authority.contains('@') {
            return Err(());
        }

        // Split host from port, requiring brackets around IPv6 literals.
        let (host, port, bracketed_host) = if let Some(rest) = authority.strip_prefix('[') {
            let close = rest.find(']').ok_or(())?;
            let host = &rest[..close];
            let suffix = &rest[close + 1..];
            let port = if suffix.is_empty() {
                None
            } else {
                Some(
                    suffix
                        .strip_prefix(':')
                        .ok_or(())?
                        .parse::<u16>()
                        .map_err(|_| ())?,
                )
            };
            (host, port, true)
        } else {
            if authority.matches(':').count() > 1 {
                return Err(());
            }
            match authority.split_once(':') {
                Some((host, port)) => (host, Some(port.parse::<u16>().map_err(|_| ())?), false),
                None => (authority, None, false),
            }
        };

        if host.is_empty() {
            return Err(());
        }

        Ok(Url {
            scheme,
            host: String::from(host),
            bracketed_host,
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
        write!(f, "{}://", self.scheme)?;
        if self.bracketed_host {
            write!(f, "[{}]", self.host)?;
        } else {
            write!(f, "{}", self.host)?;
        }
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
    fn parse_bracketed_ipv6() {
        let url = Url::parse("wss://[2001:db8::1]:443/path").unwrap();
        assert_eq!(url.host_str(), Some("2001:db8::1"));
        assert_eq!(url.port(), Some(443));
        assert_eq!(url.to_string(), "wss://[2001:db8::1]:443/path");
    }

    #[test]
    fn invalid_url() {
        assert!(Url::parse("not-a-url").is_err());
        assert!(Url::parse("://empty").is_err());
        assert!(Url::parse("ftp://example.com").is_err());
        assert!(Url::parse("wss://user@example.com").is_err());
        assert!(Url::parse("wss://example.com:abc").is_err());
        assert!(Url::parse("wss://2001:db8::1").is_err());
        assert!(Url::parse("wss://example.com/#fragment").is_err());
    }
}
