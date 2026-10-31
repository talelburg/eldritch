//! The server's configuration is read from the environment through
//! `Config::from_lookup`. These tests pass a map in place of the process
//! environment, so none of them touches `std::env`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use server::config::{Config, ProviderCredentials, Secret};
use url::Url;

/// A lookup over the given variables, standing in for the environment.
fn lookup<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    let vars: HashMap<&str, &str> = vars.iter().copied().collect();
    move |name| vars.get(name).map(ToString::to_string)
}

/// The configuration with nothing set.
fn defaults() -> Config {
    Config::from_lookup(lookup(&[])).expect("defaults are valid")
}

#[test]
fn an_empty_environment_yields_the_dev_defaults() {
    assert_eq!(
        defaults(),
        Config {
            bind: "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
            dist_dir: PathBuf::from("crates/web/dist"),
            database_url: "sqlite:eldritch.db".to_string(),
            public_origin: None,
            github: None,
        }
    );
}

#[test]
fn eldritch_bind_sets_only_the_listen_address() {
    let config = Config::from_lookup(lookup(&[("ELDRITCH_BIND", "0.0.0.0:9000")])).unwrap();

    assert_eq!(
        config,
        Config {
            bind: "0.0.0.0:9000".parse::<SocketAddr>().unwrap(),
            ..defaults()
        }
    );
}

#[test]
fn eldritch_dist_sets_only_the_bundle_directory() {
    let config = Config::from_lookup(lookup(&[(
        "ELDRITCH_DIST",
        "/srv/eldritch/releases/abc123/dist",
    )]))
    .unwrap();

    assert_eq!(
        config,
        Config {
            dist_dir: PathBuf::from("/srv/eldritch/releases/abc123/dist"),
            ..defaults()
        }
    );
}

#[test]
fn database_url_sets_only_the_database() {
    let config = Config::from_lookup(lookup(&[(
        "DATABASE_URL",
        "sqlite:/srv/eldritch/eldritch.db",
    )]))
    .unwrap();

    assert_eq!(
        config,
        Config {
            database_url: "sqlite:/srv/eldritch/eldritch.db".to_string(),
            ..defaults()
        }
    );
}

#[test]
fn a_malformed_eldritch_bind_is_an_error_naming_it_and_its_value() {
    let error = Config::from_lookup(lookup(&[("ELDRITCH_BIND", "0.0.0.0:80000")]))
        .expect_err("port 80000 is out of range")
        .to_string();

    assert!(error.contains("ELDRITCH_BIND"), "{error}");
    assert!(error.contains("0.0.0.0:80000"), "{error}");
}

#[test]
fn an_empty_eldritch_bind_is_an_error_not_the_default() {
    let error = Config::from_lookup(lookup(&[("ELDRITCH_BIND", "")]))
        .expect_err("set but empty is not unset")
        .to_string();

    assert!(error.contains("ELDRITCH_BIND"), "{error}");
}

/// GitHub's two variables, with the public origin GitHub needs beside them.
const GITHUB_ON: [(&str, &str); 3] = [
    ("ELDRITCH_PUBLIC_ORIGIN", "http://localhost:3000"),
    ("ELDRITCH_GITHUB_CLIENT_ID", "Iv1.abc"),
    ("ELDRITCH_GITHUB_CLIENT_SECRET", "hunter2"),
];

#[test]
fn a_github_id_and_secret_beside_the_public_origin_turn_github_on() {
    let config = Config::from_lookup(lookup(&GITHUB_ON)).unwrap();

    assert_eq!(
        config,
        Config {
            public_origin: Some(Url::parse("http://localhost:3000").unwrap()),
            github: Some(ProviderCredentials {
                client_id: "Iv1.abc".to_string(),
                client_secret: Secret::new("hunter2".to_string()),
            }),
            ..defaults()
        }
    );
}

#[test]
fn the_public_origin_alone_turns_no_provider_on() {
    let config = Config::from_lookup(lookup(&[(
        "ELDRITCH_PUBLIC_ORIGIN",
        "https://eldritch.gripe",
    )]))
    .unwrap();

    assert_eq!(
        config,
        Config {
            public_origin: Some(Url::parse("https://eldritch.gripe").unwrap()),
            ..defaults()
        }
    );
}

#[test]
fn a_github_id_without_its_secret_is_an_error_naming_the_secret() {
    let error = Config::from_lookup(lookup(&GITHUB_ON[..2]))
        .expect_err("half a pair")
        .to_string();

    assert!(error.contains("ELDRITCH_GITHUB_CLIENT_SECRET"), "{error}");
}

#[test]
fn a_github_secret_without_its_id_is_an_error_naming_the_id_and_not_the_secret() {
    let error = Config::from_lookup(lookup(&[GITHUB_ON[0], GITHUB_ON[2]]))
        .expect_err("half a pair")
        .to_string();

    assert!(error.contains("ELDRITCH_GITHUB_CLIENT_ID"), "{error}");
    assert!(!error.contains("hunter2"), "{error}");
}

#[test]
fn github_without_the_public_origin_is_an_error_naming_the_origin() {
    let error = Config::from_lookup(lookup(&GITHUB_ON[1..]))
        .expect_err("no origin to build the callback from")
        .to_string();

    assert!(error.contains("ELDRITCH_PUBLIC_ORIGIN"), "{error}");
}

#[test]
fn an_empty_github_secret_is_an_error_naming_it() {
    let error = Config::from_lookup(lookup(&[
        GITHUB_ON[0],
        GITHUB_ON[1],
        ("ELDRITCH_GITHUB_CLIENT_SECRET", ""),
    ]))
    .expect_err("set but empty is not set")
    .to_string();

    assert!(error.contains("ELDRITCH_GITHUB_CLIENT_SECRET"), "{error}");
}

#[test]
fn a_public_origin_that_is_not_an_http_origin_is_an_error_naming_it() {
    for value in [
        "localhost:3000",
        "ftp://localhost",
        "https://eldritch.gripe/game",
        "https://eldritch.gripe?x=1",
        "",
    ] {
        let error = Config::from_lookup(lookup(&[("ELDRITCH_PUBLIC_ORIGIN", value)]))
            .expect_err("not an http(s) origin")
            .to_string();

        assert!(
            error.contains("ELDRITCH_PUBLIC_ORIGIN"),
            "{value:?}: {error}"
        );
    }
}

#[test]
fn a_trailing_slash_on_the_public_origin_is_the_same_origin() {
    let config = Config::from_lookup(lookup(&[(
        "ELDRITCH_PUBLIC_ORIGIN",
        "https://eldritch.gripe/",
    )]))
    .unwrap();

    assert_eq!(
        config.public_origin,
        Some(Url::parse("https://eldritch.gripe").unwrap())
    );
}

#[test]
fn the_debug_form_never_shows_a_client_secret() {
    let config = Config::from_lookup(lookup(&GITHUB_ON)).unwrap();

    let debug = format!("{config:?}");
    assert!(!debug.contains("hunter2"), "{debug}");
    assert!(debug.contains("Iv1.abc"), "{debug}");
}
