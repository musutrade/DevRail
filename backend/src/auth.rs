//! Opaque Cookie session authentication and typed permission extractors.

use crate::access::{ActorContext, DataScope};
use crate::error::ApiError;
use crate::repositories;
use crate::AppState;
use argon2::password_hash::SaltString;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, Method};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use ipnet::IpNet;
use sha2::{Digest, Sha256};
use std::marker::PhantomData;
use std::net::{IpAddr, SocketAddr};
use subtle::ConstantTimeEq;
use time::Duration;

const SESSION_COOKIE: &str = "arc_session";
const SECURE_SESSION_COOKIE: &str = "__Host-arc_session";
const CSRF_COOKIE: &str = "arc_csrf";
const SECURE_CSRF_COOKIE: &str = "__Host-arc_csrf";
pub const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");
const MAX_FORWARDED_FOR_LENGTH: usize = 1_024;
const MAX_FORWARDED_FOR_HOPS: usize = 16;

#[derive(Debug, Clone)]
pub struct AuthSessionConfig {
    pub session_ttl_secs: i64,
    pub session_idle_timeout_secs: i64,
    pub persistent_session_ttl_secs: i64,
    pub persistent_session_idle_timeout_secs: i64,
    pub max_sessions_per_user: i64,
    pub login_max_failures: i32,
    pub login_ip_max_failures: i32,
    pub login_account_ip_max_failures: i32,
    pub login_failure_window_secs: i64,
    pub login_lockout_secs: i64,
    pub trusted_proxy_cidrs: Vec<IpNet>,
    pub secure_cookies: bool,
}

pub fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    fill_random(&mut bytes);
    hex(&bytes)
}

pub fn password_salt() -> SaltString {
    let mut bytes = [0_u8; 16];
    fill_random(&mut bytes);
    SaltString::encode_b64(&bytes).expect("16-byte password salt")
}

fn fill_random(bytes: &mut [u8]) {
    getrandom::fill(bytes).expect("operating system random source unavailable");
}

pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn resolve_client_ip(
    peer_ip: IpAddr,
    headers: &HeaderMap,
    trusted_proxy_cidrs: &[IpNet],
) -> IpAddr {
    let peer_ip = canonical_ip(peer_ip);
    if !is_trusted_proxy(peer_ip, trusted_proxy_cidrs) {
        return peer_ip;
    }

    let Some(forwarded_for) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() <= MAX_FORWARDED_FOR_LENGTH)
    else {
        return peer_ip;
    };
    let forwarded = forwarded_for
        .split(',')
        .map(parse_forwarded_ip)
        .collect::<Option<Vec<_>>>();
    let Some(forwarded) =
        forwarded.filter(|values| !values.is_empty() && values.len() <= MAX_FORWARDED_FOR_HOPS)
    else {
        return peer_ip;
    };

    let mut candidate = peer_ip;
    for forwarded_ip in forwarded.into_iter().rev() {
        if !is_trusted_proxy(candidate, trusted_proxy_cidrs) {
            return candidate;
        }
        candidate = forwarded_ip;
    }
    candidate
}

fn parse_forwarded_ip(value: &str) -> Option<IpAddr> {
    let value = value.trim();
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|socket| socket.ip()))
        .map(canonical_ip)
}

fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ipv6) => ipv6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ipv6)),
        ipv4 => ipv4,
    }
}

fn is_trusted_proxy(ip: IpAddr, trusted_proxy_cidrs: &[IpNet]) -> bool {
    trusted_proxy_cidrs
        .iter()
        .any(|network| network.contains(&ip))
}

pub fn set_session_cookies(
    mut jar: CookieJar,
    session_token: String,
    csrf_token: String,
    persistent: bool,
    ttl_secs: i64,
    secure: bool,
) -> CookieJar {
    let mut session = Cookie::build((session_cookie_name(secure), session_token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(secure);
    let mut csrf = Cookie::build((csrf_cookie_name(secure), csrf_token))
        .path("/")
        .http_only(false)
        .same_site(SameSite::Strict)
        .secure(secure);
    if persistent {
        let max_age = Duration::seconds(ttl_secs);
        session = session.max_age(max_age);
        csrf = csrf.max_age(max_age);
    }
    jar = jar.add(session.build());
    jar.add(csrf.build())
}

pub fn clear_session_cookies(mut jar: CookieJar, secure: bool) -> CookieJar {
    for (name, http_only) in [
        (session_cookie_name(secure), true),
        (csrf_cookie_name(secure), false),
    ] {
        let cookie = Cookie::build(name)
            .path("/")
            .http_only(http_only)
            .same_site(SameSite::Strict)
            .secure(secure)
            .build();
        jar = jar.remove(cookie);
    }
    jar
}

async fn authenticate(parts: &mut Parts, state: &AppState) -> Result<ActorContext, ApiError> {
    let jar = CookieJar::from_headers(&parts.headers);
    let session_token = jar
        .get(session_cookie_name(state.auth.secure_cookies))
        .map(Cookie::value)
        .filter(|value| value.len() == 64)
        .ok_or_else(ApiError::unauthorized)?;
    let context =
        repositories::auth_sessions::auth_context(&state.pool, &token_hash(session_token))
            .await
            .map_err(crate::error::db_error)?
            .ok_or_else(ApiError::unauthorized)?;

    validate_csrf(
        parts,
        &jar,
        &context.csrf_token_hash,
        state.auth.secure_cookies,
    )?;
    let data_scope = DataScope::from_database(&context.data_scope)
        .ok_or_else(|| ApiError::internal("认证上下文包含无效的数据范围"))?;
    crate::telemetry::record_authenticated_user(context.user_id);
    Ok(ActorContext {
        session_id: context.session_id,
        user_id: context.user_id,
        organization_id: context.organization_id,
        department_id: context.department_id,
        data_scope,
        permission_codes: context.permission_codes.into_iter().collect(),
    })
}

fn validate_csrf(
    parts: &Parts,
    jar: &CookieJar,
    expected_hash: &str,
    secure: bool,
) -> Result<(), ApiError> {
    if matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return Ok(());
    }
    let header_token = parts
        .headers
        .get(&CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() == 64)
        .ok_or_else(ApiError::csrf_invalid)?;
    let cookie_token = jar
        .get(csrf_cookie_name(secure))
        .map(Cookie::value)
        .filter(|value| value.len() == 64)
        .ok_or_else(ApiError::csrf_invalid)?;
    let values_match: bool = header_token
        .as_bytes()
        .ct_eq(cookie_token.as_bytes())
        .into();
    let hash_matches: bool = token_hash(header_token)
        .as_bytes()
        .ct_eq(expected_hash.as_bytes())
        .into();
    if !values_match || !hash_matches {
        return Err(ApiError::csrf_invalid());
    }
    Ok(())
}

fn session_cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_SESSION_COOKIE
    } else {
        SESSION_COOKIE
    }
}

fn csrf_cookie_name(secure: bool) -> &'static str {
    if secure {
        SECURE_CSRF_COOKIE
    } else {
        CSRF_COOKIE
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

impl FromRequestParts<AppState> for ActorContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        authenticate(parts, state).await
    }
}

pub trait PermissionRequirement {
    const CODE: &'static str;
}

pub struct RequirePermission<P> {
    pub actor: ActorContext,
    marker: PhantomData<P>,
}

impl<P> RequirePermission<P> {
    pub fn require(&self, code: &'static str) -> Result<(), ApiError> {
        if self.actor.has_permission(code) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!("缺少权限：{code}")))
        }
    }

    pub fn has(&self, code: &'static str) -> bool {
        self.actor.has_permission(code)
    }
}

impl<P> std::ops::Deref for RequirePermission<P> {
    type Target = ActorContext;

    fn deref(&self) -> &Self::Target {
        &self.actor
    }
}

impl<P> FromRequestParts<AppState> for RequirePermission<P>
where
    P: PermissionRequirement + Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = authenticate(parts, state).await?;
        if !auth.has_permission(P::CODE) {
            return Err(ApiError::forbidden(format!("缺少权限: {}", P::CODE)));
        }
        Ok(Self {
            actor: auth,
            marker: PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::SET_COOKIE;
    use axum::http::HeaderValue;
    use axum::response::IntoResponse;

    #[test]
    fn production_cookies_enforce_host_security_attributes() {
        let response = set_session_cookies(
            CookieJar::new(),
            random_token(),
            random_token(),
            true,
            3_600,
            true,
        )
        .into_response();
        let cookies = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .map(|value| value.to_str().expect("Set-Cookie header"))
            .collect::<Vec<_>>();
        let session = cookies
            .iter()
            .find(|cookie| cookie.starts_with("__Host-arc_session="))
            .expect("secure session cookie");
        let csrf = cookies
            .iter()
            .find(|cookie| cookie.starts_with("__Host-arc_csrf="))
            .expect("secure CSRF cookie");

        assert!(session.contains("HttpOnly"));
        assert!(session.contains("Secure"));
        assert!(session.contains("SameSite=Strict"));
        assert!(session.contains("Path=/"));
        assert!(session.contains("Max-Age=3600"));
        assert!(!csrf.contains("HttpOnly"));
        assert!(csrf.contains("Secure"));
        assert!(csrf.contains("SameSite=Strict"));
    }

    #[test]
    fn session_tokens_are_random_and_hashed_before_storage() {
        let first = random_token();
        let second = random_token();

        assert_eq!(first.len(), 64);
        assert_eq!(second.len(), 64);
        assert_ne!(first, second);
        assert_ne!(first, token_hash(&first));
        assert_eq!(token_hash(&first).len(), 64);
    }

    #[test]
    fn untrusted_peer_cannot_spoof_forwarded_address() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.10"));
        let trusted = vec!["10.0.0.0/8".parse().expect("trusted proxy CIDR")];

        assert_eq!(
            resolve_client_ip("203.0.113.7".parse().expect("peer IP"), &headers, &trusted),
            "203.0.113.7".parse::<IpAddr>().expect("expected IP")
        );
    }

    #[test]
    fn trusted_proxy_chain_uses_nearest_untrusted_address() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.20, 10.1.0.4"),
        );
        let trusted = vec!["10.0.0.0/8".parse().expect("trusted proxy CIDR")];

        assert_eq!(
            resolve_client_ip("10.2.0.5".parse().expect("peer IP"), &headers, &trusted),
            "198.51.100.20".parse::<IpAddr>().expect("expected IP")
        );
    }

    #[test]
    fn malformed_forwarded_chain_falls_back_to_peer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.20, invalid"),
        );
        let trusted = vec!["10.0.0.0/8".parse().expect("trusted proxy CIDR")];

        assert_eq!(
            resolve_client_ip("10.2.0.5".parse().expect("peer IP"), &headers, &trusted),
            "10.2.0.5".parse::<IpAddr>().expect("expected IP")
        );
    }
}
