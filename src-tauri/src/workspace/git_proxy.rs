use super::*;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};

pub(super) struct GitProxy {
    override_url: Option<String>,
    unsupported_socks: bool,
}

impl GitProxy {
    pub(super) fn apply(&self, cmd: &mut Command, remote: &str) {
        let Some(proxy) = &self.override_url else {
            return;
        };
        // GCM 2.7 also checks the older credential.*Proxy keys before http.proxy.
        for key in [
            "http.proxy".into(),
            format!("http.{remote}.proxy"),
            "remote.origin.proxy".into(),
            "credential.httpsProxy".into(),
            "credential.httpProxy".into(),
            format!("credential.{remote}.httpsProxy"),
            format!("credential.{remote}.httpProxy"),
        ] {
            cmd.arg("-c").arg(format!("{key}={proxy}"));
        }
    }

    pub(super) fn explain_auth_error(&self, message: String) -> String {
        if self.unsupported_socks
            && (message.contains("ServicePointManager")
                || message.contains("terminal prompts disabled"))
        {
            return "GitHub 浏览器授权需要 HTTP 或 HTTPS 代理。当前代理是 SOCKS，且未检测到同端口支持 HTTP。请在「授权和网络设置」填写代理软件提供的 HTTP / 混合端口，或使用 GitHub 令牌。".into();
        }
        message
    }
}

impl Workspace {
    pub(super) fn github_proxy(&self, repo: Option<&Path>, remote: &str) -> GitProxy {
        if !self.preferences.proxy.is_empty() {
            return GitProxy {
                override_url: Some(self.preferences.proxy.clone()),
                unsupported_socks: false,
            };
        }

        let git_config = |key| {
            self.git(repo, &["config", "--get-urlmatch", key, remote])
                .ok()
                .map(|value| value.trim().to_owned())
        };
        let transport = repo
            .and_then(|repo| {
                self.git(Some(repo), &["config", "--get", "remote.origin.proxy"])
                    .ok()
            })
            .map(|value| value.trim().to_owned())
            .or_else(|| git_config("http.proxy"))
            .or_else(inherited_curl_proxy);
        let credential = git_config("credential.httpsProxy")
            .filter(|value| !value.is_empty())
            .or_else(|| git_config("credential.httpProxy").filter(|value| !value.is_empty()))
            .or_else(|| git_config("http.proxy").filter(|value| !value.is_empty()))
            .or_else(inherited_curl_proxy)
            .or_else(|| std::env::var("GCM_HTTP_PROXY").ok());

        let socks = [credential.as_deref(), transport.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(|address| Url::parse(address).ok())
            .find(|url| matches!(url.scheme(), "socks5" | "socks5h" | "socks4" | "socks4a"));
        let override_url = socks
            .as_ref()
            .and_then(|proxy| local_http_proxy(proxy, http_connect));
        GitProxy {
            unsupported_socks: socks.is_some() && override_url.is_none(),
            override_url,
        }
    }
}

fn inherited_curl_proxy() -> Option<String> {
    [
        "https_proxy",
        "HTTPS_PROXY",
        "http_proxy",
        "all_proxy",
        "ALL_PROXY",
    ]
    .into_iter()
    .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

// Only try the HTTP side of a local mixed port. Never downgrade a remote
// proxy, or send proxy credentials using a different protocol.
fn local_http_proxy(
    proxy: &Url,
    accepts_connect: impl FnOnce(SocketAddr) -> bool,
) -> Option<String> {
    if !proxy.username().is_empty() || proxy.password().is_some() {
        return None;
    }
    let ip = match proxy.host()? {
        url::Host::Ipv4(ip) => IpAddr::V4(ip),
        url::Host::Ipv6(ip) => IpAddr::V6(ip),
        url::Host::Domain("localhost") => IpAddr::V4(Ipv4Addr::LOCALHOST),
        url::Host::Domain(host) => host.parse::<IpAddr>().ok()?,
    };
    let address = SocketAddr::new(ip, proxy.port()?);
    if !ip.is_loopback() || !accepts_connect(address) {
        return None;
    }
    Some(format!("http://{address}"))
}

fn http_connect(address: SocketAddr) -> bool {
    let timeout = Duration::from_secs(2);
    let probe = || -> io::Result<bool> {
        let mut stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(b"CONNECT github.com:443 HTTP/1.1\r\nHost: github.com:443\r\n\r\n")?;
        // Read a bounded prefix. Git and GCM still perform their normal TLS checks.
        let mut prefix = [0; 13];
        stream.read_exact(&mut prefix)?;
        Ok(&prefix == b"HTTP/1.1 200 " || &prefix == b"HTTP/1.0 200 ")
    };
    probe().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_only_verified_local_mixed_ports() {
        let local = Url::parse("socks5h://127.0.0.1:7897").unwrap();
        assert_eq!(
            local_http_proxy(&local, |_| true).as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(local_http_proxy(&local, |_| false), None);
        assert_eq!(
            local_http_proxy(&Url::parse("socks5://127.0.0.1:7897").unwrap(), |_| true).as_deref(),
            Some("http://127.0.0.1:7897")
        );
        for address in [
            "socks5://proxy.example.com:7897",
            "socks5://user@127.0.0.1:7897",
        ] {
            assert_eq!(
                local_http_proxy(&Url::parse(address).unwrap(), |_| panic!(
                    "unexpected probe"
                )),
                None
            );
        }
    }

    #[test]
    fn applies_proxy_to_git_and_gcm_for_the_repository() {
        let proxy = GitProxy {
            override_url: Some("http://127.0.0.1:7897".into()),
            unsupported_socks: false,
        };
        let mut command = Command::new("git");
        proxy.apply(&mut command, BLOG);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        for key in [
            format!("http.{BLOG}.proxy"),
            format!("credential.{BLOG}.httpsProxy"),
            "remote.origin.proxy".into(),
        ] {
            assert!(args.contains(&format!("{key}=http://127.0.0.1:7897").as_str()));
        }
    }
}
