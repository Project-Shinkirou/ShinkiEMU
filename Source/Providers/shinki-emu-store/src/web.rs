//! Embedded-webview sign-in. A provider describes where sign-in starts, which
//! store origins its cookies belong to and how a finished sign-in looks; the
//! Manager shows that page in a webview with an empty, non-persistent cookie
//! store and hands the captured cookies to the provider once the rule is met.
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// A provider's embedded-webview sign-in rule.
#[derive(Debug, PartialEq, Eq)]
pub struct WebLogin {
    /// First page shown in the webview.
    pub start_url: &'static str,
    /// Store origins whose cookies are handed to the provider. A cookie a
    /// browser would not send to any of them never leaves the webview.
    pub cookie_urls: &'static [&'static str],
    /// Cookies that exist only after sign-in; every one must be captured.
    pub session_cookies: &'static [&'static str],
    /// Page URL prefixes (scheme, host and path, ending in `/`) the store
    /// shows after sign-in; empty accepts any page.
    pub completion_urls: &'static [&'static str],
}

impl WebLogin {
    /// Whether `page` can be the page shown after sign-in. Cookies are read
    /// only while this holds.
    pub fn reached(&self, page: &str) -> bool {
        self.completion_urls.is_empty() || self.completion_urls.iter().any(|p| page.starts_with(p))
    }

    /// Keep only the cookies a browser would send to one of `cookie_urls`,
    /// one per name/domain/path.
    pub fn capture(&self, cookies: impl IntoIterator<Item = WebCookie>) -> Vec<WebCookie> {
        let urls: Vec<Url> = self
            .cookie_urls
            .iter()
            .filter_map(|url| Url::parse(url).ok())
            .collect();
        let mut kept: Vec<WebCookie> = Vec::new();
        for cookie in cookies {
            if urls.iter().any(|url| cookie.matches(url))
                && !kept.iter().any(|other| other.same_slot(&cookie))
            {
                kept.push(cookie);
            }
        }
        kept
    }

    /// Whether sign-in finished: `page` is a completion page and every
    /// session cookie is among the `captured` ones.
    pub fn is_complete(&self, page: &str, captured: &[WebCookie]) -> bool {
        self.reached(page)
            && self.session_cookies.iter().all(|name| {
                captured
                    .iter()
                    .any(|cookie| cookie.name == *name && !cookie.value.is_empty())
            })
    }

    /// The cookies to hand to the provider once sign-in on `page` finished:
    /// the scoped capture of `store`, or `None` while sign-in is incomplete.
    pub fn finish(&self, page: &str, store: Vec<WebCookie>) -> Option<Vec<WebCookie>> {
        if !self.reached(page) {
            return None;
        }
        let captured = self.capture(store);
        self.is_complete(page, &captured).then_some(captured)
    }
}

/// One cookie read from a webview cookie store.
// Deliberately no Debug: values are session credentials.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct WebCookie {
    pub name: String,
    pub value: String,
    /// Lowercase host or domain without a leading dot.
    pub domain: String,
    /// Sent to `domain` itself only, not to its subdomains.
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
}

impl WebCookie {
    /// Whether a browser sends this cookie with a request to `url` (RFC 6265 §5.4).
    pub fn matches(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        let domain = if self.host_only {
            host == self.domain
        } else {
            domain_matches(host, &self.domain)
        };
        domain
            && path_matches(url.path(), &self.path)
            && (!self.secure || url.scheme() == "https" || is_loopback(host))
    }

    /// Cookies from a `Cookie` request header value as sent to `url`. The
    /// header carries no attributes, so each is scoped to exactly that host.
    pub fn from_request_header(url: &Url, header: &str) -> Vec<Self> {
        let Some(host) = url.host_str() else {
            return Vec::new();
        };
        header
            .split(';')
            .filter_map(|pair| pair.trim().split_once('='))
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, value)| Self {
                name: name.trim().to_owned(),
                value: value.trim().to_owned(),
                domain: host.to_owned(),
                host_only: true,
                path: "/".to_owned(),
                secure: url.scheme() == "https",
                http_only: false,
            })
            .collect()
    }

    /// A cookie read from a webview cookie store. Stores report the domain
    /// without its leading dot, so host-only and domain cookies look alike;
    /// each is treated as a domain cookie (its host and subdomains), which
    /// keeps it within the domain the store accepted it for. Cookies without
    /// a name or domain cannot be scoped and are dropped.
    pub fn from_store(cookie: &cookie::Cookie<'_>) -> Option<Self> {
        let domain = cookie
            .domain()?
            .trim_start_matches('.')
            .to_ascii_lowercase();
        if domain.is_empty() || cookie.name().is_empty() {
            return None;
        }
        Some(Self {
            name: cookie.name().to_owned(),
            value: cookie.value().to_owned(),
            domain,
            host_only: false,
            path: match cookie.path() {
                Some(path) if path.starts_with('/') => path.to_owned(),
                _ => "/".to_owned(),
            },
            secure: cookie.secure().unwrap_or(false),
            http_only: cookie.http_only().unwrap_or(false),
        })
    }

    fn same_slot(&self, other: &Self) -> bool {
        self.name == other.name && self.domain == other.domain && self.path == other.path
    }
}

/// Cookies of one store session. Each request gets exactly the cookies a
/// browser would send to its URL, and responses update them.
#[derive(Default)]
pub struct CookieJar {
    cookies: Vec<WebCookie>,
}

impl CookieJar {
    pub fn new(cookies: Vec<WebCookie>) -> Self {
        Self { cookies }
    }

    pub fn cookies(&self) -> &[WebCookie] {
        &self.cookies
    }

    pub fn contains(&self, name: &str) -> bool {
        self.cookies
            .iter()
            .any(|cookie| cookie.name == name && !cookie.value.is_empty())
    }

    /// `Cookie` header value for a request to `url`; longer paths first.
    pub fn header(&self, url: &Url) -> Option<Zeroizing<String>> {
        let mut sent: Vec<&WebCookie> = self.cookies.iter().filter(|c| c.matches(url)).collect();
        if sent.is_empty() {
            return None;
        }
        sent.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        let mut header = Zeroizing::new(String::new());
        for cookie in sent {
            if !header.is_empty() {
                header.push_str("; ");
            }
            header.push_str(&cookie.name);
            header.push('=');
            header.push_str(&cookie.value);
        }
        Some(header)
    }

    /// Apply the `Set-Cookie` header values of a response to `url`. Cookies
    /// for a domain the response host may not set are ignored.
    pub fn update<'a>(&mut self, url: &Url, set_cookie: impl IntoIterator<Item = &'a str>) {
        let Some(host) = url.host_str() else {
            return;
        };
        for text in set_cookie {
            let Ok(parsed) = cookie::Cookie::parse(text) else {
                continue;
            };
            let (domain, host_only) = match parsed.domain().map(str::to_ascii_lowercase) {
                Some(domain) if domain.contains('.') && domain_matches(host, &domain) => {
                    (domain, false)
                }
                Some(_) => continue,
                None => (host.to_owned(), true),
            };
            let path = match parsed.path() {
                Some(path) if path.starts_with('/') => path.to_owned(),
                _ => default_path(url.path()),
            };
            let expired = parsed
                .max_age()
                .is_some_and(|age| age.is_zero() || age.is_negative())
                || parsed
                    .expires_datetime()
                    .is_some_and(|at| at <= cookie::time::OffsetDateTime::now_utc());
            let cookie = WebCookie {
                name: parsed.name().to_owned(),
                value: parsed.value().to_owned(),
                domain,
                host_only,
                path,
                secure: parsed.secure().unwrap_or(false),
                http_only: parsed.http_only().unwrap_or(false),
            };
            self.cookies.retain(|old| !old.same_slot(&cookie));
            if !expired {
                self.cookies.push(cookie);
            }
        }
    }
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn path_matches(request: &str, cookie: &str) -> bool {
    request == cookie
        || request
            .strip_prefix(cookie)
            .is_some_and(|rest| cookie.ends_with('/') || rest.starts_with('/'))
}

fn default_path(request: &str) -> String {
    match request.rfind('/') {
        Some(0) | None => "/".to_owned(),
        Some(end) => request[..end].to_owned(),
    }
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    static DLSITE: WebLogin = WebLogin {
        start_url: "https://play.dlsite.com/login/",
        cookie_urls: &["https://play.dlsite.com/", "https://login.dlsite.com/"],
        session_cookies: &["__DLsite_SID"],
        completion_urls: &["https://play.dlsite.com/"],
    };

    fn cookie(name: &str, domain: &str, host_only: bool, path: &str) -> WebCookie {
        WebCookie {
            name: name.into(),
            value: format!("{name}-value"),
            domain: domain.into(),
            host_only,
            path: path.into(),
            secure: true,
            http_only: true,
        }
    }
    fn names(cookies: &[WebCookie]) -> Vec<&str> {
        cookies.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn capture_keeps_only_cookies_sent_to_store_origins() {
        let captured = DLSITE.capture([
            cookie("__DLsite_SID", "dlsite.com", false, "/"),
            cookie("play_session", "play.dlsite.com", true, "/"),
            cookie("www_only", "www.dlsite.com", true, "/"),
            cookie("lookalike", "evildlsite.com", false, "/"),
            cookie("tracker", "google.com", false, "/"),
            cookie("deep", "play.dlsite.com", true, "/admin"),
            cookie("__DLsite_SID", "dlsite.com", false, "/"),
        ]);
        assert_eq!(names(&captured), ["__DLsite_SID", "play_session"]);
    }

    #[test]
    fn insecure_origins_never_receive_secure_cookies() {
        let url = Url::parse("http://play.dlsite.com/").unwrap();
        assert!(!cookie("a", "play.dlsite.com", true, "/").matches(&url));
        let local = Url::parse("http://127.0.0.1:8080/").unwrap();
        assert!(cookie("a", "127.0.0.1", true, "/").matches(&local));
    }

    #[test]
    fn completion_needs_the_page_and_every_session_cookie() {
        let signed_in = [cookie("__DLsite_SID", "dlsite.com", false, "/")];
        assert!(DLSITE.is_complete("https://play.dlsite.com/library", &signed_in));
        assert!(!DLSITE.is_complete("https://login.dlsite.com/login", &signed_in));
        assert!(!DLSITE.is_complete("https://play.dlsite.com.evil.example/", &signed_in));
        assert!(!DLSITE.is_complete("https://play.dlsite.com/", &[]));
        let mut empty = cookie("__DLsite_SID", "dlsite.com", false, "/");
        empty.value.clear();
        assert!(!DLSITE.is_complete("https://play.dlsite.com/", &[empty]));
        let any_page = WebLogin {
            completion_urls: &[],
            ..DLSITE
        };
        assert!(any_page.is_complete("https://example.com/", &signed_in));
    }

    #[test]
    fn jar_scopes_headers_and_applies_responses() {
        let mut jar = CookieJar::new(vec![
            cookie("sid", "dlsite.com", false, "/"),
            cookie("play", "play.dlsite.com", true, "/"),
        ]);
        let play = Url::parse("https://play.dlsite.com/api/authorize").unwrap();
        let www = Url::parse("https://www.dlsite.com/home/").unwrap();
        assert_eq!(
            jar.header(&play).unwrap().as_str(),
            "sid=sid-value; play=play-value"
        );
        assert_eq!(jar.header(&www).unwrap().as_str(), "sid=sid-value");
        jar.update(
            &play,
            [
                "play=fresh; Path=/; Secure; HttpOnly",
                "sid=; Max-Age=0; Domain=.dlsite.com; Path=/",
                "foreign=x; Domain=example.com",
                "scoped=y; Path=/api",
            ],
        );
        assert_eq!(jar.header(&www), None);
        assert_eq!(jar.header(&play).unwrap().as_str(), "scoped=y; play=fresh");
        assert!(!jar.contains("sid"));
        assert!(jar.contains("play"));
    }

    #[test]
    fn request_headers_become_host_scoped_cookies() {
        let url = Url::parse("https://accounts.dmm.com/").unwrap();
        let cookies =
            WebCookie::from_request_header(&url, "login_secure_id=a; login_session_id=b;  ");
        assert_eq!(names(&cookies), ["login_secure_id", "login_session_id"]);
        assert!(
            cookies
                .iter()
                .all(|c| c.host_only && c.domain == "accounts.dmm.com")
        );
        assert!(!cookies[0].matches(&Url::parse("https://www.dmm.com/").unwrap()));
    }

    #[test]
    fn store_cookies_keep_scope_and_drop_unscoped_ones() {
        let parse = |text: &str| WebCookie::from_store(&cookie::Cookie::parse(text).unwrap());
        let session = parse("__DLsite_SID=s; Domain=.DLsite.com; HttpOnly; Secure").unwrap();
        assert_eq!(session.domain, "dlsite.com");
        assert!(session.http_only && session.secure && !session.host_only);
        assert_eq!(session.path, "/");
        assert!(session.matches(&Url::parse("https://play.dlsite.com/library").unwrap()));
        assert!(!session.matches(&Url::parse("https://notdlsite.com/").unwrap()));
        let scoped = parse("a=b; Domain=play.dlsite.com; Path=/api").unwrap();
        assert!(!scoped.matches(&Url::parse("https://play.dlsite.com/").unwrap()));
        assert!(scoped.matches(&Url::parse("http://play.dlsite.com/api/v3").unwrap()));
        assert!(parse("a=b").is_none());
        let unnamed = cookie::Cookie::build(("", "b"))
            .domain("dlsite.com")
            .build();
        assert!(WebCookie::from_store(&unnamed).is_none());
    }

    #[test]
    fn finish_hands_over_only_scoped_cookies_of_a_completed_sign_in() {
        let store = || {
            vec![
                cookie("__DLsite_SID", "dlsite.com", false, "/"),
                cookie("tracker", "google.com", false, "/"),
            ]
        };
        assert!(
            DLSITE
                .finish("https://login.dlsite.com/login", store())
                .is_none()
        );
        assert!(DLSITE.finish("https://play.dlsite.com/", vec![]).is_none());
        let handed = DLSITE.finish("https://play.dlsite.com/", store()).unwrap();
        assert_eq!(names(&handed), ["__DLsite_SID"]);
    }
}
