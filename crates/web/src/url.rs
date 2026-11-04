//! The client's addresses: a game's link, read from and written to the page's
//! path, and its same-origin WebSocket URL (D3): never a hardcoded port.

use protocol::GameId;

/// The path every game's address starts with: a game's link is
/// `/games/{id}`.
const GAMES_PREFIX: &str = "/games/";

/// The game a page path names: `Some` for `/games/{id}`, `None` for any other
/// path, `/` included.
#[must_use]
pub fn game_id_from_path(path: &str) -> Option<GameId> {
    let id = path.strip_prefix(GAMES_PREFIX)?.trim_end_matches('/');
    (!id.is_empty() && !id.contains('/')).then(|| GameId::new(id))
}

/// A game's address, as a same-origin path.
#[must_use]
pub fn game_path(game_id: &GameId) -> String {
    format!("{GAMES_PREFIX}{}", game_id.as_str())
}

/// A game's link, the one an invitation sends: its [`game_path`] under
/// `origin` (`window.location.origin`, scheme, host and port).
#[must_use]
pub fn game_link(origin: &str, game_id: &GameId) -> String {
    format!("{origin}{}", game_path(game_id))
}

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

/// The browser's `window.location`.
///
/// # Panics
///
/// Panics if there is no browser `window` (e.g. called outside a DOM context) —
/// always present in the wasm client this targets.
#[cfg(target_arch = "wasm32")]
fn location() -> web_sys::Location {
    web_sys::window().expect("a browser window").location()
}

/// Read `window.location` and build this game's WebSocket URL.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn current_ws_url(game_id: &str) -> String {
    let loc = location();
    let protocol = loc.protocol().unwrap_or_else(|_| "http:".to_string());
    let host = loc.host().unwrap_or_default();
    ws_url(&protocol, &host, game_id)
}

/// The game this page's path names, if it names one.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn current_game_id() -> Option<GameId> {
    game_id_from_path(&location().pathname().ok()?)
}

/// This page's link to `game_id`, under the page's own origin.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn current_game_link(game_id: &GameId) -> String {
    game_link(&location().origin().unwrap_or_default(), game_id)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_games_path_names_its_game() {
        assert_eq!(
            game_id_from_path("/games/0b6f3f9e-2d1c"),
            Some(GameId::new("0b6f3f9e-2d1c"))
        );
        assert_eq!(game_id_from_path("/games/g1/"), Some(GameId::new("g1")));
    }

    #[test]
    fn any_other_path_names_no_game() {
        for path in ["/", "", "/games", "/games/", "/games/g1/more", "/game/g1"] {
            assert_eq!(game_id_from_path(path), None, "{path}");
        }
    }

    #[test]
    fn a_games_link_is_its_path_under_the_origin() {
        let id = GameId::new("g1");
        assert_eq!(game_path(&id), "/games/g1");
        assert_eq!(
            game_link("https://play.example.com", &id),
            "https://play.example.com/games/g1"
        );
        assert_eq!(game_id_from_path(&game_path(&id)), Some(id));
    }
}
