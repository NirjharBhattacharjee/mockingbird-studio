//! The whole Google sign-in against a fake Google on 127.0.0.1: the consent
//! page, the loopback redirect, the code exchange with PKCE, the revoke.
//! The Keychain is a folder here (`MOCKINGBIRD_STUDIO_KEYCHAIN_DIR`), kept
//! apart from every folder the test searches for the tokens.

mod common;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{BIN, Client};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CLIENT_ID: &str = "test-client.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "test-client-secret";
const CODE: &str = "test-code-4/0Ab";
const ACCESS: &str = "ya29.test-access-token-never-shown";
const REFRESH: &str = "1//test-refresh-token-never-shown";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube.upload";

#[derive(Default)]
struct Google {
    challenge: String,
    redirect: String,
    exchanged: bool,
    revoked: Vec<String>,
}

fn form(s: &str) -> HashMap<String, String> {
    form_urlencoded::parse(s.as_bytes()).into_owned().collect()
}

fn reply(mut s: &TcpStream, status: &str, body: &str) {
    write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

/// The browser following Google's redirect back to the app. Returns the
/// status line the app answered with.
fn browser_back(redirect: &str, query: &str) -> String {
    let addr = redirect.trim_start_matches("http://");
    let mut s = TcpStream::connect(addr).unwrap();
    write!(s, "GET /?{query} HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.lines().next().unwrap_or_default().to_string()
}

fn handle(s: TcpStream, g: &Mutex<Google>) {
    let mut r = BufReader::new(&s);
    let mut first = String::new();
    r.read_line(&mut first).unwrap();
    let mut len = 0;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        if h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':')
            && k.eq_ignore_ascii_case("content-length")
        {
            len = v.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).unwrap();
    let body = form(std::str::from_utf8(&body).unwrap());
    let target = first.split_whitespace().nth(1).unwrap();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        // What the browser opens: Google's consent page. The user says yes.
        "/auth" => {
            let q = form(query);
            assert_eq!(q["client_id"], CLIENT_ID);
            assert_eq!(q["scope"], SCOPE, "the upload scope and nothing else");
            assert_eq!(q["response_type"], "code");
            assert_eq!(q["code_challenge_method"], "S256");
            assert_eq!(q["access_type"], "offline");
            assert!(q["redirect_uri"].starts_with("http://127.0.0.1:"));
            {
                let mut g = g.lock().unwrap();
                g.challenge = q["code_challenge"].clone();
                g.redirect = q["redirect_uri"].clone();
            }
            reply(&s, "200 OK", "{}");
            drop(s);
            let wrong = browser_back(&q["redirect_uri"], "code=evil&state=wrong");
            assert!(
                wrong.contains("404"),
                "a wrong state is turned away: {wrong}"
            );
            let back = browser_back(
                &q["redirect_uri"],
                &format!("code={}&state={}", CODE.replace('/', "%2F"), q["state"]),
            );
            assert!(back.contains("200"), "{back}");
        }
        "/token" => {
            let mut g = g.lock().unwrap();
            assert_eq!(body["grant_type"], "authorization_code");
            assert_eq!(body["code"], CODE);
            assert_eq!(body["client_id"], CLIENT_ID);
            assert_eq!(body["client_secret"], CLIENT_SECRET);
            assert_eq!(body["redirect_uri"], g.redirect);
            let hashed = URL_SAFE_NO_PAD.encode(Sha256::digest(body["code_verifier"].as_bytes()));
            assert_eq!(
                hashed, g.challenge,
                "the PKCE verifier matches its challenge"
            );
            g.exchanged = true;
            let tokens = json!({
                "access_token": ACCESS, "refresh_token": REFRESH, "expires_in": 3599,
                "scope": SCOPE, "token_type": "Bearer",
            });
            reply(&s, "200 OK", &tokens.to_string());
        }
        "/revoke" => {
            g.lock().unwrap().revoked.push(body["token"].clone());
            reply(&s, "200 OK", "");
        }
        _ => reply(&s, "404 Not Found", "{}"),
    }
}

fn fake_google() -> (String, Arc<Mutex<Google>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let g = Arc::new(Mutex::new(Google::default()));
    let g2 = g.clone();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            handle(s.unwrap(), &g2);
        }
    });
    (base, g)
}

/// Every file under `dir`, read as bytes.
fn files(dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push((p.display().to_string(), std::fs::read(&p).unwrap()));
        }
    }
}

#[test]
fn a_sign_in_runs_end_to_end_and_no_reply_file_or_log_holds_the_token() {
    let home = tempfile::tempdir().unwrap();
    let keychain = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let client_file = elsewhere.path().join("client_secret.json");
    let client = json!({"installed": {
        "client_id": CLIENT_ID, "client_secret": CLIENT_SECRET,
        "auth_uri": "https://accounts.google.com/o/oauth2/auth",
        "token_uri": "https://oauth2.googleapis.com/token",
        "redirect_uris": ["http://localhost"],
    }});
    std::fs::write(&client_file, client.to_string()).unwrap();
    let (base, google) = fake_google();
    let env = [
        ("MOCKINGBIRD_STUDIO_HOME", home.path().to_str().unwrap()),
        (
            "MOCKINGBIRD_STUDIO_KEYCHAIN_DIR",
            keychain.path().to_str().unwrap(),
        ),
        ("MOCKINGBIRD_STUDIO_GOOGLE", base.as_str()),
        // Stands in for the browser: it opens the consent page.
        ("MOCKINGBIRD_STUDIO_BROWSER", "/usr/bin/curl"),
    ];
    // Every reply and every line of log, searched for the tokens at the end.
    let mut seen = String::new();
    let run = |seen: &mut String, args: &[&str]| -> Value {
        let out = Command::new(BIN)
            .args(args)
            .arg("--json")
            .envs(env)
            .output()
            .unwrap();
        let (stdout, stderr) = (
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        *seen += &stdout;
        *seen += &stderr;
        assert!(out.status.success(), "{args:?}: {stderr}");
        serde_json::from_str(&stdout).unwrap()
    };

    let r = run(
        &mut seen,
        &[
            "youtube-connect",
            "--label",
            "main",
            "--client-file",
            client_file.to_str().unwrap(),
        ],
    );
    assert_eq!(r["signed_in"], true);
    assert!(google.lock().unwrap().exchanged);
    let item = std::fs::read_to_string(keychain.path().join("main")).unwrap();
    assert!(
        item.contains(REFRESH),
        "the refresh token is in the Keychain"
    );
    assert!(!item.contains(ACCESS), "the access token isn't kept");

    let mut mcp = Client::start_env(home.path(), &env);
    let (status, line) = mcp.call("youtube_status", json!({}));
    seen += &format!("{status}\n{line}");
    assert_eq!(status["channels"][0]["label"], "main");
    assert_eq!(status["channels"][0]["signed_in"], true);
    let tools = mcp.request("tools/list", json!({}));
    for name in ["youtube_connect", "youtube_status", "youtube_disconnect"] {
        assert!(tools.to_string().contains(name), "{name}");
    }
    drop(mcp);

    let r = run(&mut seen, &["youtube-disconnect", "--label", "main"]);
    assert_eq!(r["revoked"], true);
    assert_eq!(google.lock().unwrap().revoked, vec![REFRESH.to_string()]);
    assert!(!keychain.path().join("main").exists());
    let r = run(&mut seen, &["youtube-status"]);
    assert_eq!(r["channels"][0]["signed_in"], false);

    let mut all = Vec::new();
    files(home.path(), &mut all);
    files(elsewhere.path(), &mut all);
    assert!(!all.is_empty());
    for token in [ACCESS, REFRESH] {
        assert!(!seen.contains(token), "a reply or log holds {token}");
        for (path, bytes) in &all {
            let text = String::from_utf8_lossy(bytes);
            assert!(!text.contains(token), "{path} holds {token}");
        }
    }
}

#[test]
fn a_refused_consent_is_an_error_and_keeps_nothing() {
    // Google sends the browser back with error=access_denied.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let s = s.unwrap();
            let mut first = String::new();
            BufReader::new(&s).read_line(&mut first).unwrap();
            let q = form(
                first
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .split_once('?')
                    .unwrap()
                    .1,
            );
            reply(&s, "200 OK", "{}");
            drop(s);
            browser_back(
                &q["redirect_uri"],
                &format!("error=access_denied&state={}", q["state"]),
            );
        }
    });
    let home = tempfile::tempdir().unwrap();
    let keychain = tempfile::tempdir().unwrap();
    let client_file = home.path().join("client.json");
    let client = json!({"installed": {"client_id": CLIENT_ID, "client_secret": CLIENT_SECRET}});
    std::fs::write(&client_file, client.to_string()).unwrap();
    let out = Command::new(BIN)
        .args(["youtube-connect", "--label", "main", "--client-file"])
        .arg(&client_file)
        .env("MOCKINGBIRD_STUDIO_HOME", home.path())
        .env("MOCKINGBIRD_STUDIO_KEYCHAIN_DIR", keychain.path())
        .env("MOCKINGBIRD_STUDIO_GOOGLE", &base)
        .env("MOCKINGBIRD_STUDIO_BROWSER", "/usr/bin/curl")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("access_denied"));
    assert_eq!(std::fs::read_dir(keychain.path()).unwrap().count(), 0);
}
