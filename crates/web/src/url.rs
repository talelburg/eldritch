//! Same-origin WebSocket URL derivation (D3): never a hardcoded port.

/// Build the same-origin WebSocket URL for a game. `location_protocol`
/// is `window.location.protocol` (`"http:"` / `"https:"`); `host`
/// includes the port. Upgrades to `wss` under TLS; otherwise `ws`.
pub fn ws_url(location_protocol: &str, host: &str, game_id: &str) -> String {
    let scheme = if location_protocol == "https:" {
        "wss"
    } else {
        "ws"
    };
    format!("{scheme}://{host}/ws/{game_id}")
}

/// Read `window.location` and build this game's WebSocket URL.
///
/// # Panics
///
/// Panics if there is no browser `window` (e.g. called outside a DOM context) —
/// always present in the wasm client this targets.
#[cfg(target_arch = "wasm32")]
pub fn current_ws_url(game_id: &str) -> String {
    let loc = web_sys::window().expect("a browser window").location();
    let protocol = loc.protocol().unwrap_or_else(|_| "http:".to_string());
    let host = loc.host().unwrap_or_default();
    ws_url(&protocol, &host, game_id)
}

#[cfg(test)]
mod tests {
    use super::ws_url;

    #[test]
    fn plain_http_uses_ws_and_keeps_port() {
        assert_eq!(
            ws_url("http:", "localhost:3000", "abc"),
            "ws://localhost:3000/ws/abc"
        );
    }

    #[test]
    fn https_upgrades_to_wss() {
        assert_eq!(
            ws_url("https:", "play.example.com", "g1"),
            "wss://play.example.com/ws/g1"
        );
    }
}
