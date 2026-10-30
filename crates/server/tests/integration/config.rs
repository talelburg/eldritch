//! The server's configuration is read from the environment through
//! `Config::from_lookup`. These tests pass a map in place of the process
//! environment, so none of them touches `std::env`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use server::config::Config;

/// A lookup over the given variables, standing in for the environment.
fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    let vars: HashMap<&str, &str> = vars.iter().copied().collect();
    move |name| vars.get(name).map(ToString::to_string)
}

#[test]
fn an_empty_environment_yields_the_dev_defaults() {
    let config = Config::from_lookup(env(&[])).expect("defaults are valid");

    assert_eq!(config.bind, "127.0.0.1:8000".parse::<SocketAddr>().unwrap());
    assert_eq!(config.dist_dir, PathBuf::from("crates/web/dist"));
    assert_eq!(config.database_url, "sqlite:eldritch.db");
}

#[test]
fn eldritch_bind_sets_only_the_listen_address() {
    let config = Config::from_lookup(env(&[("ELDRITCH_BIND", "0.0.0.0:9000")])).unwrap();

    assert_eq!(config.bind, "0.0.0.0:9000".parse::<SocketAddr>().unwrap());
    assert_eq!(config.dist_dir, PathBuf::from("crates/web/dist"));
    assert_eq!(config.database_url, "sqlite:eldritch.db");
}

#[test]
fn eldritch_dist_sets_only_the_bundle_directory() {
    let config = Config::from_lookup(env(&[(
        "ELDRITCH_DIST",
        "/srv/eldritch/releases/abc123/dist",
    )]))
    .unwrap();

    assert_eq!(config.bind, "127.0.0.1:8000".parse::<SocketAddr>().unwrap());
    assert_eq!(
        config.dist_dir,
        PathBuf::from("/srv/eldritch/releases/abc123/dist")
    );
    assert_eq!(config.database_url, "sqlite:eldritch.db");
}

#[test]
fn database_url_sets_only_the_database() {
    let config =
        Config::from_lookup(env(&[("DATABASE_URL", "sqlite:/srv/eldritch/eldritch.db")])).unwrap();

    assert_eq!(config.bind, "127.0.0.1:8000".parse::<SocketAddr>().unwrap());
    assert_eq!(config.dist_dir, PathBuf::from("crates/web/dist"));
    assert_eq!(config.database_url, "sqlite:/srv/eldritch/eldritch.db");
}

#[test]
fn a_malformed_eldritch_bind_is_an_error_naming_it_and_its_value() {
    let error = Config::from_lookup(env(&[("ELDRITCH_BIND", "0.0.0.0:80000")]))
        .expect_err("port 80000 is out of range")
        .to_string();

    assert!(error.contains("ELDRITCH_BIND"), "{error}");
    assert!(error.contains("0.0.0.0:80000"), "{error}");
}

#[test]
fn an_empty_eldritch_bind_is_an_error_not_the_default() {
    let error = Config::from_lookup(env(&[("ELDRITCH_BIND", "")]))
        .expect_err("set but empty is not unset")
        .to_string();

    assert!(error.contains("ELDRITCH_BIND"), "{error}");
}
