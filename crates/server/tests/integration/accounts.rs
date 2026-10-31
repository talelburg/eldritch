//! The `server accounts` subcommand keeps the allowlist: run as the built
//! binary against a temporary `SQLite` file, the way the host runs it over ssh.

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use axum::http::{header, StatusCode};
use server::db::{self, MIGRATOR};
use server::AppState;

use crate::common::{self, TempDb};

/// Run the built `server` binary with `args` against `db`.
///
/// `RUST_LOG=debug` makes the server log everything it would, so a test
/// reading `list`'s stdout also checks no log line lands there.
fn server(db: &TempDb, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_server"))
        .args(args)
        .env("DATABASE_URL", db.url())
        .env("RUST_LOG", "debug")
        .output()
        .expect("run the server binary")
}

/// Run `server accounts <args>` and require it to succeed.
fn accounts(db: &TempDb, args: &[&str]) -> Output {
    let output = server(db, &[&["accounts"], args].concat());
    assert!(
        output.status.success(),
        "`server accounts {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Run `server accounts <args>` and require it to fail; return its stderr.
fn refused(db: &TempDb, args: &[&str]) -> String {
    let output = server(db, &[&["accounts"], args].concat());
    assert!(
        !output.status.success(),
        "`server accounts {}` should have been refused",
        args.join(" ")
    );
    String::from_utf8(output.stderr).expect("utf-8 stderr")
}

/// `server accounts list`'s stdout.
fn list(db: &TempDb) -> String {
    String::from_utf8(accounts(db, &["list"]).stdout).expect("utf-8 stdout")
}

#[test]
fn an_added_account_is_listed() {
    let db = TempDb::new("accounts");

    accounts(&db, &["add", "--name", "Sam", "sam@gmail.com"]);

    assert_eq!(list(&db), "1\tSam\tsam@gmail.com\tenabled\n");
}

#[test]
fn an_email_another_entry_holds_is_refused_naming_that_entry() {
    let db = TempDb::new("accounts");
    accounts(&db, &["add", "--name", "Sam", "sam@gmail.com"]);

    for email in ["sam@gmail.com", "SAM@gmail.com"] {
        let stderr = refused(&db, &["add", "--name", "Alex", "alex@x.com", email]);
        assert!(
            stderr.contains("sam@gmail.com is already on entry 1 (Sam)"),
            "refusal should name Sam's entry: {stderr}"
        );
    }

    // Nothing of a refused entry is kept, not even its other email.
    assert_eq!(list(&db), "1\tSam\tsam@gmail.com\tenabled\n");
}

#[test]
fn one_entry_holds_several_emails_lowercased() {
    let db = TempDb::new("accounts");

    accounts(
        &db,
        &["add", "--name", "Sam", "Sam@Work.com", "sam@gmail.com"],
    );

    assert_eq!(list(&db), "1\tSam\tsam@gmail.com, sam@work.com\tenabled\n");
}

#[test]
fn a_removed_entry_is_listed_disabled() {
    let db = TempDb::new("accounts");
    accounts(&db, &["add", "--name", "Sam", "sam@gmail.com"]);
    accounts(&db, &["add", "--name", "Alex", "alex@x.com"]);

    accounts(&db, &["remove", "SAM@gmail.com"]);
    accounts(&db, &["remove", "2"]);

    assert_eq!(
        list(&db),
        "1\tSam\tsam@gmail.com\tdisabled\n2\tAlex\talex@x.com\tdisabled\n"
    );
}

/// `remove` reads an email the way `add` stored it: trimmed and lowercased.
#[test]
fn removing_by_a_padded_mixed_case_email_disables_its_entry() {
    let db = TempDb::new("accounts");
    accounts(&db, &["add", "--name", "Sam", "sam@gmail.com"]);

    accounts(&db, &["remove", " SAM@gmail.com "]);

    assert_eq!(list(&db), "1\tSam\tsam@gmail.com\tdisabled\n");
}

#[test]
fn removing_an_entry_nobody_holds_is_refused() {
    let db = TempDb::new("accounts");
    accounts(&db, &["add", "--name", "Sam", "sam@gmail.com"]);

    let by_email = refused(&db, &["remove", "nobody@x.com"]);
    assert!(
        by_email.contains("no entry matches nobody@x.com"),
        "{by_email}"
    );
    let by_id = refused(&db, &["remove", "7"]);
    assert!(by_id.contains("no entry matches 7"), "{by_id}");

    assert_eq!(list(&db), "1\tSam\tsam@gmail.com\tenabled\n");
}

/// Removing an entry ends its sessions: the cookie Sam signed in with fails
/// on Sam's next request and on Sam's next connect. Alex, signed in too, is
/// untouched.
#[tokio::test]
async fn removing_an_entry_ends_its_sessions() {
    let db = TempDb::new("accounts-sessions");
    let pool = db::connect_pool(&db.url()).await.expect("open the file");
    MIGRATOR.run(&pool).await.expect("migrate");
    common::create_game(&pool, "g-removed").await;
    accounts(&db, &["add", "--name", "Sam", "sam@gmail.com"]);
    accounts(&db, &["add", "--name", "Alex", "alex@x.com"]);
    let addr = common::serve(server::app(
        AppState::new(pool, common::REGISTRIES).with_dev_auth(true),
    ))
    .await;
    let sam = common::sign_in_over_http(addr, "Sam").await;
    let alex = common::sign_in_over_http(addr, "Alex").await;

    accounts(&db, &["remove", "sam@gmail.com"]);

    assert_eq!(me(addr, &sam).await, StatusCode::UNAUTHORIZED);
    let mut ws = common::connect_with(addr, "g-removed", Some(&sam)).await;
    common::assert_closed_unauthenticated(&mut ws).await;
    assert_eq!(me(addr, &alex).await, StatusCode::OK);
}

/// The status of `GET /auth/me` at `addr` with `cookie`.
async fn me(addr: SocketAddr, cookie: &str) -> StatusCode {
    common::http()
        .get(format!("http://{addr}/auth/me"))
        .header(header::COOKIE, cookie)
        .send()
        .await
        .expect("GET /auth/me")
        .status()
}

#[test]
fn with_no_subcommand_the_binary_serves() {
    let db = TempDb::new("accounts-serve");

    let responses = serve_binary(&db, &[], &["/health", "/auth/dev"]);

    assert!(
        responses[0].starts_with("HTTP/1.1 200"),
        "/health should answer 200: {}",
        responses[0]
    );
    assert!(
        responses[1].starts_with("HTTP/1.1 404"),
        "without --dev-auth the dev picker is absent: {}",
        responses[1]
    );
}

#[test]
fn with_dev_auth_the_binary_serves_the_dev_picker() {
    let db = TempDb::new("accounts-dev-auth");
    accounts(&db, &["add", "--name", "Sam", "sam@gmail.com"]);

    let responses = serve_binary(&db, &["--dev-auth"], &["/auth/dev"]);

    assert!(
        responses[0].starts_with("HTTP/1.1 200") && responses[0].contains(">Sam</button>"),
        "the dev picker should list Sam: {}",
        responses[0]
    );
}

/// Start the built binary with `args` against `db`, `GET` each of `paths`
/// once it answers, stop it, and return the raw responses in order.
fn serve_binary(db: &TempDb, args: &[&str], paths: &[&str]) -> Vec<String> {
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("pick a free port")
        .port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_server"))
        .args(args)
        .env("DATABASE_URL", db.url())
        .env("ELDRITCH_BIND", format!("127.0.0.1:{port}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the server binary");

    let deadline = Instant::now() + Duration::from_secs(30);
    while get(port, "/health").is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the server never answered on port {port}");
        }
        thread::sleep(Duration::from_millis(50));
    }
    let responses = paths
        .iter()
        .map(|path| get(port, path).expect("the server answers"))
        .collect();
    child.kill().expect("stop the server");
    child.wait().expect("reap the server");
    responses
}

/// `GET path` on `port` with a loopback `Host`, or `None` while nothing is
/// listening.
fn get(port: u16, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}
