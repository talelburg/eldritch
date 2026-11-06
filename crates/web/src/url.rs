//! The client's addresses: a game's link, the home page's, a sign-in
//! method's, and a game's WebSocket URL.
//!
//! A game's link is read from and written to the page's path. The home
//! page's `?ended=` query names the [`Ended`] that sent a client there. A
//! sign-in method's link names the page to return to. The WebSocket URL is
//! same-origin (D3): never a hardcoded port.

use protocol::GameId;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;
#[cfg(target_arch = "wasm32")]
use web_sys::Location;

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

/// The query key naming the ending "Your games" was reached after.
const ENDED_KEY: &str = "ended";

/// An ending that sends a client from its game to "Your games", which then
/// says why. It travels in the home page's address, `/?ended=…`, because the
/// move is a page load, which starts a fresh store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The account left the lobby, or the creator removed it.
    NoLongerMember,
    /// The creator deleted the game.
    Deleted,
}

impl Ended {
    /// Every ending.
    const ALL: [Ended; 2] = [Ended::NoLongerMember, Ended::Deleted];

    /// The query value naming this ending.
    fn query_value(self) -> &'static str {
        match self {
            Ended::NoLongerMember => "not-member",
            Ended::Deleted => "deleted",
        }
    }

    /// The toast "Your games" shows after this ending.
    #[must_use]
    pub fn toast(self) -> &'static str {
        match self {
            Ended::NoLongerMember => "You're no longer a member of this game",
            Ended::Deleted => "This game was deleted",
        }
    }
}

/// The home page's path, naming `ended` if a client is sent there after one.
#[must_use]
pub fn home_path(ended: Option<Ended>) -> String {
    match ended {
        Some(ended) => format!("/?{ENDED_KEY}={}", ended.query_value()),
        None => "/".to_string(),
    }
}

/// The ending a page's query (`window.location.search`, `?` included) names,
/// if it names one.
#[must_use]
pub fn ended_from_query(query: &str) -> Option<Ended> {
    let value = query
        .strip_prefix('?')
        .unwrap_or(query)
        .split('&')
        .find_map(|pair| pair.strip_prefix(ENDED_KEY)?.strip_prefix('='))?;
    Ended::ALL
        .into_iter()
        .find(|ended| ended.query_value() == value)
}

/// The query key naming the page a sign-in returns to. The server reads it,
/// and lands on `/` for anything that isn't a path on this site.
const RETURN_TO_KEY: &str = "return_to";

/// The link to a sign-in method at `method_url` that returns to `page` (the
/// page's path and query) once it succeeds. From `/`, where a sign-in lands
/// anyway, it is `method_url` unchanged.
#[must_use]
pub fn sign_in_link(method_url: &str, page: &str) -> String {
    if page == "/" {
        return method_url.to_string();
    }
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair(RETURN_TO_KEY, page)
        .finish();
    format!("{method_url}?{query}")
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
fn location() -> Location {
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

/// The ending this page's address names, if it names one. Reading it takes it
/// out of the address, in place, so a reload or a later visit to this history
/// entry doesn't say it again.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn take_current_ended() -> Option<Ended> {
    let ended = ended_from_query(&location().search().ok()?)?;
    let path = location().pathname().ok()?;
    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&path));
    }
    Some(ended)
}

/// The link to a sign-in method at `method_url` that returns to this page.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn current_sign_in_link(method_url: &str) -> String {
    let location = location();
    let page = format!(
        "{}{}",
        location.pathname().unwrap_or_else(|_| "/".to_string()),
        location.search().unwrap_or_default()
    );
    sign_in_link(method_url, &page)
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
    fn home_after_an_ending_names_it_in_the_query() {
        assert_eq!(home_path(Some(Ended::NoLongerMember)), "/?ended=not-member");
        assert_eq!(home_path(Some(Ended::Deleted)), "/?ended=deleted");
        assert_eq!(home_path(None), "/");
    }

    #[test]
    fn the_query_names_the_ending_home_was_reached_after() {
        assert_eq!(
            ended_from_query("?ended=not-member"),
            Some(Ended::NoLongerMember)
        );
        assert_eq!(
            ended_from_query("?x=1&ended=not-member"),
            Some(Ended::NoLongerMember)
        );
        assert_eq!(ended_from_query("?ended=deleted"), Some(Ended::Deleted));
        for query in ["", "?", "?ended=", "?ended=bogus", "?not-member", "?x=1"] {
            assert_eq!(ended_from_query(query), None, "{query}");
        }
    }

    #[test]
    fn not_a_member_says_so() {
        assert_eq!(
            Ended::NoLongerMember.toast(),
            "You're no longer a member of this game"
        );
    }

    #[test]
    fn deleted_says_so() {
        assert_eq!(Ended::Deleted.toast(), "This game was deleted");
    }

    #[test]
    fn a_sign_in_link_names_the_page_it_started_from() {
        assert_eq!(
            sign_in_link("/auth/github/login", "/games/abc"),
            "/auth/github/login?return_to=%2Fgames%2Fabc"
        );
        assert_eq!(
            sign_in_link("/auth/dev", "/games/abc?x=1"),
            "/auth/dev?return_to=%2Fgames%2Fabc%3Fx%3D1"
        );
    }

    #[test]
    fn a_sign_in_link_from_home_is_the_method_s_own() {
        assert_eq!(
            sign_in_link("/auth/github/login", "/"),
            "/auth/github/login"
        );
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
