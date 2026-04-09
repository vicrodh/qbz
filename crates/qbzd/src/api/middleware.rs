//! LAN-only access restriction.
//!
//! Rejects HTTP requests from non-private IP addresses.
//! If you're on the same network, you're in. If you're not, you're out.
//! Remote control from outside the LAN is exclusively via QConnect (cloud).

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::net::IpAddr;

/// Middleware: reject requests from non-private IPs.
/// ConnectInfo is injected by axum::serve with into_make_service_with_connect_info.
pub async fn lan_only(
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract client IP from ConnectInfo extension (set by axum::serve)
    if let Some(addr) = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>() {
        if !is_private_ip(&addr.0.ip()) {
            log::warn!(
                "[qbzd] Rejected non-LAN request from {} to {}",
                addr.0.ip(),
                req.uri().path()
            );
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(next.run(req).await)
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
            || v4.is_private()
            || v4.is_link_local()
            || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
            || v6.is_unspecified()
            || (v6.segments()[0] & 0xffc0) == 0xfe80  // link-local
            || (v6.segments()[0] & 0xff00) == 0xfd00   // unique local (LAN)
        }
    }
}
