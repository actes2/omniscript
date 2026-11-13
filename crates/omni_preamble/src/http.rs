//! Cross-Platform In-Process HTTP/1.1 + TLS 1.2/1.3 Client
//!
//! Built directly on `std::net::TcpStream` and pure-Rust `rustls` (`ring` + Mozilla root CAs)
//! to guarantee identical handshake, redirect, and framing semantics across Linux, macOS, and Windows
//! without shelling out to OS-dependent `curl` binaries.

use crate::PreambleError;
use rustls::pki_types::ServerName;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone)]
struct ParsedUrl {
    is_https: bool,
    host: String,
    port: u16,
    path_and_query: String,
}

impl ParsedUrl {
    fn parse(raw: &str) -> Result<Self, PreambleError> {
        let trimmed = raw.trim();
        let (is_https, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = trimmed.strip_prefix("http://") {
            (false, r)
        } else {
            return Err(PreambleError::PathNotFound(format!(
                "Unsupported URL scheme in '{}'",
                trimmed
            )));
        };

        let default_port = if is_https { 443 } else { 80 };
        let (authority, path_and_query) = match rest.find('/') {
            Some(idx) => (&rest[..idx], rest[idx..].to_string()),
            None => (rest, "/".to_string()),
        };

        let (host, port) = if authority.starts_with('[') {
            // IPv6 literal `[::1]:8080`
            if let Some(end_bracket) = authority.find(']') {
                let h = &authority[1..end_bracket];
                let rem = &authority[end_bracket + 1..];
                let p = if let Some(p_str) = rem.strip_prefix(':') {
                    p_str.parse::<u16>().unwrap_or(default_port)
                } else {
                    default_port
                };
                (h.to_string(), p)
            } else {
                return Err(PreambleError::PathNotFound(format!(
                    "Malformed IPv6 authority in '{}'",
                    trimmed
                )));
            }
        } else if let Some((h, p_str)) = authority.rsplit_once(':') {
            let p = p_str.parse::<u16>().map_err(|_| {
                PreambleError::PathNotFound(format!("Invalid port in URL '{}'", trimmed))
            })?;
            (h.to_string(), p)
        } else {
            (authority.to_string(), default_port)
        };

        if host.is_empty() {
            return Err(PreambleError::PathNotFound(format!(
                "Empty host in URL '{}'",
                trimmed
            )));
        }

        Ok(Self {
            is_https,
            host,
            port,
            path_and_query,
        })
    }

    fn host_header(&self) -> String {
        let default_port = if self.is_https { 443 } else { 80 };
        if self.port == default_port {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn resolve_redirect(&self, location: &str) -> String {
        let loc = location.trim();
        if loc.starts_with("https://") || loc.starts_with("http://") {
            loc.to_string()
        } else if let Some(rest) = loc.strip_prefix("//") {
            let scheme = if self.is_https { "https" } else { "http" };
            format!("{}://{}", scheme, rest)
        } else if loc.starts_with('/') {
            let scheme = if self.is_https { "https" } else { "http" };
            format!("{}://{}{}", scheme, self.host_header(), loc)
        } else {
            let scheme = if self.is_https { "https" } else { "http" };
            let base_dir = match self.path_and_query.rfind('/') {
                Some(idx) => &self.path_and_query[..=idx],
                None => "/",
            };
            format!("{}://{}{}{}", scheme, self.host_header(), base_dir, loc)
        }
    }
}

fn shared_tls_config() -> Arc<rustls::ClientConfig> {
    static TLS_CFG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    TLS_CFG
        .get_or_init(|| {
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let cfg = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("valid TLS protocol versions")
                .with_root_certificates(root_store)
                .with_no_client_auth();
            Arc::new(cfg)
        })
        .clone()
}

enum HttpTransport {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for HttpTransport {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            HttpTransport::Plain(s) => s.read(buf),
            HttpTransport::Tls(s) => s.read(buf),
        }
    }
}

impl Write for HttpTransport {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            HttpTransport::Plain(s) => s.write(buf),
            HttpTransport::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            HttpTransport::Plain(s) => s.flush(),
            HttpTransport::Tls(s) => s.flush(),
        }
    }
}

fn connect_transport(
    parsed: &ParsedUrl,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> Result<HttpTransport, PreambleError> {
    let addrs: Vec<_> = (parsed.host.as_str(), parsed.port)
        .to_socket_addrs()
        .map_err(|e| {
            PreambleError::PathNotFound(format!(
                "DNS resolution failed for '{}:{}': {}",
                parsed.host, parsed.port, e
            ))
        })?
        .collect();

    if addrs.is_empty() {
        return Err(PreambleError::PathNotFound(format!(
            "No socket addresses resolved for '{}'",
            parsed.host
        )));
    }

    let mut last_err = None;
    let mut connected_tcp = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, connect_timeout) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(io_timeout));
                let _ = stream.set_write_timeout(Some(io_timeout));
                connected_tcp = Some(stream);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }

    let tcp = connected_tcp.ok_or_else(|| {
        PreambleError::PathNotFound(format!(
            "TCP connection to '{}:{}' failed: {:?}",
            parsed.host, parsed.port, last_err
        ))
    })?;

    if parsed.is_https {
        let server_name = ServerName::try_from(parsed.host.clone()).map_err(|e| {
            PreambleError::PathNotFound(format!("Invalid TLS SNI host '{}': {}", parsed.host, e))
        })?;
        let tls_conn = rustls::ClientConnection::new(shared_tls_config(), server_name).map_err(
            |e| PreambleError::PathNotFound(format!("TLS handshake init failed: {}", e)),
        )?;
        let mut tls_stream = rustls::StreamOwned::new(tls_conn, tcp);
        // Complete TLS handshake immediately so handshake errors report deterministically
        while tls_stream.conn.is_handshaking() {
            if let Err(e) = tls_stream.conn.complete_io(&mut tls_stream.sock) {
                return Err(PreambleError::PathNotFound(format!(
                    "TLS handshake failed with '{}': {}",
                    parsed.host, e
                )));
            }
        }
        Ok(HttpTransport::Tls(Box::new(tls_stream)))
    } else {
        Ok(HttpTransport::Plain(tcp))
    }
}

/// Perform an HTTP/1.1 GET request over `std::net::TcpStream` (+ `rustls` for HTTPS),
/// following up to 10 redirects and streaming the decoded response body into `sink`.
pub fn http_get_to_writer<W: Write>(
    url: &str,
    headers: &[(&str, &str)],
    connect_timeout: Duration,
    io_timeout: Duration,
    sink: &mut W,
) -> Result<(), PreambleError> {
    let mut current_url = url.to_string();
    let mut active_headers: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    for _hop in 0..=MAX_REDIRECTS {
        let parsed = ParsedUrl::parse(&current_url)?;
        let mut transport = connect_transport(&parsed, connect_timeout, io_timeout)?;

        // Synthesize raw HTTP/1.1 request header block
        let mut req = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: omnic/{}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n",
            parsed.path_and_query,
            parsed.host_header(),
            env!("CARGO_PKG_VERSION")
        );
        for (k, v) in &active_headers {
            req.push_str(&format!("{}: {}\r\n", k, v));
        }
        req.push_str("\r\n");

        transport.write_all(req.as_bytes()).map_err(|e| {
            PreambleError::PathNotFound(format!("HTTP write failed for '{}': {}", current_url, e))
        })?;
        transport.flush().map_err(|e| {
            PreambleError::PathNotFound(format!("HTTP flush failed for '{}': {}", current_url, e))
        })?;

        let mut reader = BufReader::with_capacity(64 * 1024, transport);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).map_err(|e| {
            PreambleError::PathNotFound(format!(
                "Failed reading HTTP status from '{}': {}",
                current_url, e
            ))
        })?;

        let status_code: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| {
                PreambleError::PathNotFound(format!(
                    "Malformed HTTP status line {:?} from '{}'",
                    status_line.trim(),
                    current_url
                ))
            })?;

        let mut location: Option<String> = None;
        let mut is_chunked = false;
        let mut content_length: Option<u64> = None;

        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).map_err(|e| {
                PreambleError::PathNotFound(format!(
                    "Failed reading HTTP headers from '{}': {}",
                    current_url, e
                ))
            })?;
            if n == 0 || line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_ascii_lowercase();
                let val = v.trim();
                match key.as_str() {
                    "location" => location = Some(val.to_string()),
                    "transfer-encoding" if val.to_ascii_lowercase().contains("chunked") => {
                        is_chunked = true;
                    }
                    "content-length" => {
                        content_length = val.parse::<u64>().ok();
                    }
                    _ => {}
                }
            }
        }

        if matches!(status_code, 301 | 302 | 303 | 307 | 308) {
            let loc = location.ok_or_else(|| {
                PreambleError::PathNotFound(format!(
                    "HTTP {} redirect missing Location header from '{}'",
                    status_code, current_url
                ))
            })?;
            let next_url = parsed.resolve_redirect(&loc);
            if let Ok(next_parsed) = ParsedUrl::parse(&next_url) {
                // RFC 9110 §15.4: strip Authorization header across cross-origin redirects
                if !next_parsed.host.eq_ignore_ascii_case(&parsed.host) {
                    active_headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
                }
            }
            current_url = next_url;
            continue;
        }

        if !(200..300).contains(&status_code) {
            return Err(PreambleError::PathNotFound(format!(
                "HTTP {} returned for '{}'",
                status_code, current_url
            )));
        }

        if is_chunked {
            read_chunked_body(&mut reader, sink, &current_url)?;
        } else if let Some(expected_len) = content_length {
            let mut limited = reader.take(expected_len);
            let copied = copy_ignore_close_notify(&mut limited, sink).map_err(|e| {
                PreambleError::PathNotFound(format!(
                    "HTTP body stream interrupted from '{}': {}",
                    current_url, e
                ))
            })?;
            if copied < expected_len {
                return Err(PreambleError::PathNotFound(format!(
                    "Truncated HTTP body from '{}' (got {}/{} bytes)",
                    current_url, copied, expected_len
                )));
            }
        } else {
            copy_ignore_close_notify(&mut reader, sink).map_err(|e| {
                PreambleError::PathNotFound(format!(
                    "HTTP body read failed from '{}': {}",
                    current_url, e
                ))
            })?;
        }

        sink.flush().map_err(|e| {
            PreambleError::PathNotFound(format!("Failed flushing HTTP output sink: {}", e))
        })?;
        return Ok(());
    }

    Err(PreambleError::PathNotFound(format!(
        "Exceeded maximum HTTP redirects ({}) for '{}'",
        MAX_REDIRECTS, url
    )))
}

/// Copy bytes until EOF, tolerating servers that close TCP cleanly without sending a TLS `close_notify` alert
/// after `Connection: close` (RFC 8446 / widespread CDN behavior).
fn copy_ignore_close_notify<R: Read, W: Write + ?Sized>(
    reader: &mut R,
    writer: &mut W,
) -> io::Result<u64> {
    let mut buf = [0u8; 32 * 1024];
    let mut total = 0u64;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => {
                writer.write_all(&buf[..n])?;
                total += n as u64;
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(total),
            Err(e) => return Err(e),
        }
    }
}

fn read_chunked_body<R: BufRead, W: Write + ?Sized>(
    reader: &mut R,
    sink: &mut W,
    url: &str,
) -> Result<(), PreambleError> {
    loop {
        let mut size_line = String::new();
        let n = reader.read_line(&mut size_line).map_err(|e| {
            PreambleError::PathNotFound(format!("Chunk header read error from '{}': {}", url, e))
        })?;
        if n == 0 {
            break;
        }
        let hex_part = size_line
            .trim()
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if hex_part.is_empty() {
            continue;
        }
        let chunk_size = usize::from_str_radix(hex_part, 16).map_err(|_| {
            PreambleError::PathNotFound(format!(
                "Invalid HTTP chunk size {:?} from '{}'",
                hex_part, url
            ))
        })?;
        if chunk_size == 0 {
            // Consume optional trailer headers + final CRLF
            let mut trailer = String::new();
            let _ = reader.read_line(&mut trailer);
            break;
        }
        let mut limited = reader.take(chunk_size as u64);
        io::copy(&mut limited, sink).map_err(|e| {
            PreambleError::PathNotFound(format!("Chunk payload read error from '{}': {}", url, e))
        })?;
        let mut crlf = [0u8; 2];
        let _ = reader.read_exact(&mut crlf);
    }
    Ok(())
}

pub fn fetch_bytes(url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>, PreambleError> {
    let mut out = Vec::new();
    http_get_to_writer(
        url,
        headers,
        Duration::from_secs(8),
        Duration::from_secs(20),
        &mut out,
    )?;
    Ok(out)
}

pub fn fetch_string(url: &str, headers: &[(&str, &str)]) -> Result<String, PreambleError> {
    let bytes = fetch_bytes(url, headers)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn download_to_path(
    url: &str,
    dest: &Path,
    headers: &[(&str, &str)],
) -> Result<(), PreambleError> {
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp_dest = dest.with_extension("part");
    let res = (|| -> Result<(), PreambleError> {
        let mut file = fs::File::create(&tmp_dest).map_err(|e| {
            PreambleError::PathNotFound(format!("Failed creating {:?}: {}", tmp_dest, e))
        })?;
        http_get_to_writer(
            url,
            headers,
            Duration::from_secs(10),
            Duration::from_secs(90),
            &mut file,
        )?;
        Ok(())
    })();

    match res {
        Ok(()) => {
            if dest.exists() {
                let _ = fs::remove_file(dest);
            }
            fs::rename(&tmp_dest, dest).map_err(|e| {
                let _ = fs::remove_file(&tmp_dest);
                PreambleError::PathNotFound(format!(
                    "Failed finalizing download {:?}: {}",
                    dest, e
                ))
            })
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp_dest);
            Err(e)
        }
    }
}
