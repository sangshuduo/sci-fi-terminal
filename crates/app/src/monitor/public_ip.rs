//! Opt-in discovery of this machine's public IP address (ADR-007).
//!
//! This is the application's only network request, and it is OFF by
//! default. When enabled, one HTTPS GET goes to a user-configurable endpoint
//! that returns the caller's address as plain text. The endpoint necessarily
//! learns the user's IP; the settings UI says so. The response is bounded,
//! validated, and never executed or rendered as markup.

use std::net::IpAddr;
use std::time::Duration;

use super::connections::is_public;

/// Default endpoint: returns the caller's public IP as plain text.
pub const DEFAULT_ENDPOINT: &str = "https://api.ipify.org";
/// Minimum time between lookups.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Time allowed for the whole request.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Largest response accepted; an IPv6 address plus whitespace fits easily.
const MAX_BODY_BYTES: u64 = 64;
const MAX_ENDPOINT_LEN: usize = 256;

/// Only plain `https://` URLs without whitespace or control characters.
pub fn validate_endpoint(endpoint: &str) -> Result<(), String> {
    let trimmed = endpoint.trim();
    if trimmed.len() > MAX_ENDPOINT_LEN {
        return Err(format!(
            "endpoint is longer than {MAX_ENDPOINT_LEN} characters"
        ));
    }
    if !trimmed.starts_with("https://") || trimmed.len() <= "https://".len() {
        return Err("endpoint must be an https:// URL".into());
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("endpoint must not contain spaces or control characters".into());
    }
    Ok(())
}

/// Parse a response body into a public IP address.
pub fn parse_body(body: &str) -> Result<IpAddr, String> {
    let ip: IpAddr = body
        .trim()
        .parse()
        .map_err(|_| "the endpoint did not return an IP address".to_owned())?;
    if !is_public(ip) {
        return Err(format!("{ip} is not a public address"));
    }
    Ok(ip)
}

/// Blocking fetch; call only from a background worker.
pub fn fetch(endpoint: &str) -> Result<IpAddr, String> {
    validate_endpoint(endpoint)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(0)
        .https_only(true)
        .build()
        .into();
    let mut response = agent
        .get(endpoint.trim())
        .header("Accept", "text/plain")
        .call()
        .map_err(|err| format!("public IP lookup failed: {err}"))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_string()
        .map_err(|err| format!("public IP lookup failed: {err}"))?;
    parse_body(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_must_be_https_and_clean() {
        assert!(validate_endpoint(DEFAULT_ENDPOINT).is_ok());
        assert!(validate_endpoint("http://api.ipify.org").is_err());
        assert!(validate_endpoint("https://").is_err());
        assert!(validate_endpoint("file:///etc/passwd").is_err());
        assert!(validate_endpoint("https://a b").is_err());
        assert!(validate_endpoint(&format!("https://{}", "a".repeat(300))).is_err());
    }

    #[test]
    fn bodies_must_be_a_single_public_address() {
        assert_eq!(
            parse_body(" 93.184.216.34\n"),
            Ok("93.184.216.34".parse().expect("ip"))
        );
        assert!(parse_body("2606:4700:4700::1111").is_ok());
        assert!(parse_body("192.168.1.10").is_err());
        assert!(parse_body("<html>hello</html>").is_err());
        assert!(parse_body("").is_err());
    }

    #[test]
    fn invalid_endpoint_is_rejected_before_any_request() {
        assert!(fetch("http://example.com").is_err());
    }
}
