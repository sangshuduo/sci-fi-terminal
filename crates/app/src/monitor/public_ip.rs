//! Opt-in discovery of this machine's public IP and location (ADR-007).
//!
//! This is the application's only network request, and it is OFF by
//! default. When enabled, one HTTPS GET goes to a user-configurable endpoint.
//! Two response shapes are accepted:
//!
//! - JSON with the caller's IP and location (the default, ipinfo.io, and
//!   compatible services). The service's location is used directly, which is
//!   usually more accurate for the user's own address than a free offline
//!   database (ADR-009).
//! - Plain text containing only the IP (e.g. api.ipify.org); the offline
//!   GeoIP database then locates it.
//!
//! The endpoint necessarily learns the user's IP; the settings UI says so.
//! The response is bounded, validated field by field, and never executed or
//! rendered as markup.

use std::net::IpAddr;
use std::time::Duration;

use serde_json::Value;

use super::connections::is_public;
use super::geoip::GeoLocation;

/// Default endpoint: returns the caller's IP and location as JSON.
pub const DEFAULT_ENDPOINT: &str = "https://ipinfo.io/json";
/// Minimum time between lookups.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Time allowed for the whole request.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Largest response accepted.
const MAX_BODY_BYTES: u64 = 4 * 1024;
const MAX_ENDPOINT_LEN: usize = 256;
/// Longest place name kept from a response.
const MAX_NAME_CHARS: usize = 64;

/// The caller's public address and, if the endpoint reported one, its location.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicIpInfo {
    pub ip: IpAddr,
    pub location: Option<GeoLocation>,
    /// Host that supplied `location`, for on-screen attribution.
    pub source: String,
}

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

/// Host part of an endpoint URL, used to attribute the location.
pub fn endpoint_host(endpoint: &str) -> String {
    let rest = endpoint.trim().trim_start_matches("https://");
    rest.split(['/', '?', '#', ':'])
        .next()
        .unwrap_or(rest)
        .to_owned()
}

/// Parse a response body: JSON with location, or plain text with only an IP.
pub fn parse_body(body: &str, source: &str) -> Result<PublicIpInfo, String> {
    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        return parse_json(trimmed, source);
    }
    Ok(PublicIpInfo {
        ip: parse_ip(trimmed)?,
        location: None,
        source: source.to_owned(),
    })
}

fn parse_ip(text: &str) -> Result<IpAddr, String> {
    let ip: IpAddr = text
        .trim()
        .parse()
        .map_err(|_| "the endpoint did not return an IP address".to_owned())?;
    if !is_public(ip) {
        return Err(format!("{ip} is not a public address"));
    }
    Ok(ip)
}

/// Accepts common field names across services (ipinfo, ipapi-style, ip-api-style).
fn parse_json(body: &str, source: &str) -> Result<PublicIpInfo, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| "the endpoint returned invalid JSON".to_owned())?;
    let ip_text = first_str(&value, &["ip", "query", "ipAddress"])
        .ok_or_else(|| "the endpoint's JSON has no IP address".to_owned())?;
    let ip = parse_ip(&ip_text)?;
    let (latitude, longitude) = coordinates(&value);
    let location = GeoLocation {
        country_code: first_str(&value, &["country_code", "countryCode", "country"])
            .filter(|code| code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic())),
        country: first_str(&value, &["country_name", "countryName"]),
        city: first_str(&value, &["city", "cityName"]),
        region: first_str(&value, &["region", "regionName", "region_name"]),
        latitude,
        longitude,
    };
    let useful = location.city.is_some() || location.latitude.is_some();
    Ok(PublicIpInfo {
        ip,
        location: useful.then_some(location),
        source: source.to_owned(),
    })
}

/// First present string field, stripped of control characters and bounded.
fn first_str(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(key)?.as_str())
        .and_then(|text| {
            let clean: String = text
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_NAME_CHARS)
                .collect();
            let clean = clean.trim().to_owned();
            (!clean.is_empty()).then_some(clean)
        })
}

/// `latitude`/`longitude` (or `lat`/`lon`) numbers, or ipinfo's `"lat,lon"` string.
fn coordinates(value: &Value) -> (Option<f64>, Option<f64>) {
    let number = |keys: &[&str]| keys.iter().find_map(|key| value.get(key)?.as_f64());
    let pair = match (number(&["latitude", "lat"]), number(&["longitude", "lon"])) {
        (Some(lat), Some(lon)) => Some((lat, lon)),
        _ => value.get("loc").and_then(Value::as_str).and_then(|loc| {
            let (lat, lon) = loc.split_once(',')?;
            Some((lat.trim().parse().ok()?, lon.trim().parse().ok()?))
        }),
    };
    match pair {
        Some((lat, lon))
            if lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0 =>
        {
            (Some(lat), Some(lon))
        }
        _ => (None, None),
    }
}

/// Blocking fetch; call only from a background worker.
pub fn fetch(endpoint: &str) -> Result<PublicIpInfo, String> {
    validate_endpoint(endpoint)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(0)
        .https_only(true)
        .build()
        .into();
    let mut response = agent
        .get(endpoint.trim())
        .header("Accept", "application/json, text/plain")
        .call()
        .map_err(|err| format!("public IP lookup failed: {err}"))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY_BYTES)
        .read_to_string()
        .map_err(|err| format!("public IP lookup failed: {err}"))?;
    parse_body(&body, &endpoint_host(endpoint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_must_be_https_and_clean() {
        assert!(validate_endpoint(DEFAULT_ENDPOINT).is_ok());
        assert!(validate_endpoint("https://api.ipify.org").is_ok());
        assert!(validate_endpoint("http://api.ipify.org").is_err());
        assert!(validate_endpoint("https://").is_err());
        assert!(validate_endpoint("file:///etc/passwd").is_err());
        assert!(validate_endpoint("https://a b").is_err());
        assert!(validate_endpoint(&format!("https://{}", "a".repeat(300))).is_err());
    }

    #[test]
    fn host_is_extracted_for_attribution() {
        assert_eq!(endpoint_host("https://ipinfo.io/json"), "ipinfo.io");
        assert_eq!(endpoint_host("https://example.com:8443/x?y"), "example.com");
    }

    #[test]
    fn plain_text_bodies_carry_only_the_ip() {
        let info = parse_body(" 93.184.216.34\n", "api.ipify.org").expect("parse");
        assert_eq!(info.ip, "93.184.216.34".parse::<IpAddr>().expect("ip"));
        assert_eq!(info.location, None);
        assert!(parse_body("2606:4700:4700::1111", "x").is_ok());
        assert!(parse_body("192.168.1.10", "x").is_err());
        assert!(parse_body("<html>hello</html>", "x").is_err());
        assert!(parse_body("", "x").is_err());
    }

    #[test]
    fn ipinfo_style_json_is_located() {
        let body = r#"{"ip":"172.97.129.229","city":"Toronto","region":"Ontario",
            "country":"CA","loc":"43.7064,-79.3986","org":"AS1 Example"}"#;
        let info = parse_body(body, "ipinfo.io").expect("parse");
        let location = info.location.expect("location");
        assert_eq!(location.city.as_deref(), Some("Toronto"));
        assert_eq!(location.region.as_deref(), Some("Ontario"));
        assert_eq!(location.country_code.as_deref(), Some("CA"));
        assert_eq!(
            (location.latitude, location.longitude),
            (Some(43.7064), Some(-79.3986))
        );
        assert_eq!(info.source, "ipinfo.io");
    }

    #[test]
    fn numeric_coordinate_json_is_located() {
        let body = r#"{"query":"93.184.216.34","city":"Norwell","regionName":"Massachusetts",
            "countryCode":"US","lat":42.15,"lon":-70.82}"#;
        let location = parse_body(body, "x")
            .expect("parse")
            .location
            .expect("location");
        assert_eq!(location.city.as_deref(), Some("Norwell"));
        assert_eq!(
            (location.latitude, location.longitude),
            (Some(42.15), Some(-70.82))
        );
    }

    #[test]
    fn hostile_or_broken_json_is_bounded_or_rejected() {
        assert!(parse_body(r#"{"city":"Toronto"}"#, "x").is_err(), "no IP");
        assert!(
            parse_body(r#"{"ip":"10.0.0.1"}"#, "x").is_err(),
            "private IP"
        );
        assert!(parse_body("{not json", "x").is_err());
        let long = "A".repeat(500);
        let body = format!(r#"{{"ip":"8.8.8.8","city":"{long}\u001b[31m","loc":"999,0"}}"#);
        let location = parse_body(&body, "x")
            .expect("parse")
            .location
            .expect("location");
        assert_eq!(
            location.city.map(|c| c.chars().count()),
            Some(MAX_NAME_CHARS)
        );
        assert_eq!(location.latitude, None, "out-of-range coordinates dropped");
        let bare = parse_body(r#"{"ip":"8.8.8.8"}"#, "x").expect("parse");
        assert_eq!(
            bare.location, None,
            "no useful location → fall back to the database"
        );
    }

    #[test]
    fn invalid_endpoint_is_rejected_before_any_request() {
        assert!(fetch("http://example.com").is_err());
    }
}
