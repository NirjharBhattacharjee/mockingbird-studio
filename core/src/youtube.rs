//! Signing in to the owner's own YouTube channel: the browser, PKCE, a
//! loopback redirect on 127.0.0.1, the `youtube.upload` scope only, and the
//! refresh token in the macOS Keychain. No function here returns a token,
//! prints one or writes one to a file.
//!
//! Three variables exist for the tests, which touch neither Google nor the
//! Keychain: `MOCKINGBIRD_STUDIO_GOOGLE` points the endpoints at a server on
//! 127.0.0.1, `MOCKINGBIRD_STUDIO_BROWSER` is run with the consent address
//! instead of `open`, and `MOCKINGBIRD_STUDIO_KEYCHAIN_DIR` keeps each item in
//! a file in that folder instead of the Keychain.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::project::Result;
use crate::store;

const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const REVOKE: &str = "https://oauth2.googleapis.com/revoke";
pub const SCOPE: &str = "https://www.googleapis.com/auth/youtube.upload";
const SERVICE: &str = "mockingbird-studio";
const WAIT: Duration = Duration::from_secs(5 * 60);

/// A `[[channel]]` in config.toml. Nothing secret: the token is in the Keychain.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct Channel {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// What the Keychain holds under a label. No `Debug`, so it can't be logged
/// by accident.
#[derive(Serialize, Deserialize)]
struct Secret {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

/// Google's address, or the test server's on 127.0.0.1.
fn endpoint(real: &str) -> String {
    match std::env::var("MOCKINGBIRD_STUDIO_GOOGLE") {
        Ok(base) if base.starts_with("http://127.0.0.1:") => {
            format!("{base}/{}", real.rsplit('/').next().unwrap_or_default())
        }
        _ => real.to_string(),
    }
}

fn check_label(label: &str) -> Result<()> {
    let ok = !label.is_empty()
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!("label {label:?} should be letters, digits, - or _"))
    }
}

fn keychain_dir() -> Option<PathBuf> {
    std::env::var_os("MOCKINGBIRD_STUDIO_KEYCHAIN_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
}

fn save(label: &str, secret: &Secret) -> Result<()> {
    let bytes = serde_json::to_vec(secret).map_err(|e| e.to_string())?;
    match keychain_dir() {
        Some(dir) => fs::write(dir.join(label), bytes).map_err(|e| e.to_string()),
        None => security_framework::passwords::set_generic_password(SERVICE, label, &bytes)
            .map_err(|e| format!("can't save to the Keychain: {e}")),
    }
}

fn load(label: &str) -> Result<Option<Secret>> {
    let bytes = match keychain_dir() {
        Some(dir) => match fs::read(dir.join(label)) {
            Ok(b) => b,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        },
        None => match security_framework::passwords::get_generic_password(SERVICE, label) {
            Ok(b) => b,
            // errSecItemNotFound
            Err(e) if e.code() == -25300 => {
                return Ok(None);
            }
            Err(e) => return Err(format!("can't read the Keychain: {e}")),
        },
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| format!("the Keychain item for {label} is not one this app wrote"))
}

fn delete(label: &str) -> Result<()> {
    match keychain_dir() {
        Some(dir) => fs::remove_file(dir.join(label)).map_err(|e| e.to_string()),
        None => security_framework::passwords::delete_generic_password(SERVICE, label)
            .map_err(|e| format!("can't delete from the Keychain: {e}")),
    }
}

/// The client id and secret from the JSON file Google gives for a Desktop
/// app client. Errors never quote the file.
fn read_client(path: &str) -> Result<(String, String)> {
    let text = fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| format!("{path} is not a JSON file"))?;
    let c = &v["installed"];
    match (c["client_id"].as_str(), c["client_secret"].as_str()) {
        (Some(id), Some(secret)) => Ok((id.to_string(), secret.to_string())),
        _ => Err(format!(
            "{path} is not a Desktop app client's file. Make the OAuth client with the type Desktop app (docs/YOUTUBE.md)"
        )),
    }
}

fn random(n: usize) -> Result<String> {
    let mut bytes = vec![0; n];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("can't read random bytes: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// A form POST to Google. An error carries only Google's `error` fields,
/// never the body, which could hold a token.
fn post(url: &str, form: &[(&str, &str)]) -> Result<Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut resp = agent
        .post(endpoint(url))
        .send_form(form.iter().copied())
        .map_err(|e| format!("can't reach Google: {e}"))?;
    let status = resp.status();
    let body: Value = resp
        .body_mut()
        .read_to_string()
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    if status.is_success() {
        Ok(body)
    } else {
        Err(format!(
            "Google answered {status}: {} {}",
            body["error"].as_str().unwrap_or_default(),
            body["error_description"].as_str().unwrap_or_default()
        )
        .trim_end()
        .to_string())
    }
}

fn open_browser(url: &str) -> Result<()> {
    // The address holds no secret: the PKCE challenge and the state only.
    eprintln!("Opening Google's sign-in page. If no browser opens, go to:\n{url}");
    let program = std::env::var("MOCKINGBIRD_STUDIO_BROWSER").unwrap_or_else(|_| "open".into());
    let mut child = Command::new(&program)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("can't run {program}: {e}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Waits up to five minutes for the browser to come back with the code.
/// Requests without the right state are turned away and the wait goes on.
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    let io = |e: std::io::Error| format!("the sign-in listener failed: {e}");
    listener.set_nonblocking(true).map_err(io)?;
    let deadline = Instant::now() + WAIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(r) = answer(stream, state) {
                    return r;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err("no sign-in came back within five minutes".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(io(e)),
        }
    }
}

fn answer(mut stream: TcpStream, state: &str) -> Option<Result<String>> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut reader = BufReader::new(&stream);
    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    // Read the headers too, so closing doesn't reset the connection.
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line.trim_end() != "" {
        line.clear();
    }
    let target = first.split_whitespace().nth(1).unwrap_or_default();
    let query = target.split_once('?').map_or("", |(_, q)| q);
    let (mut code, mut got, mut error) = (None, None, None);
    for (k, v) in form_urlencoded::parse(query.as_bytes()) {
        match &*k {
            "code" => code = Some(v.into_owned()),
            "state" => got = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            _ => {}
        }
    }
    let (status, body, result) = match (got.as_deref() == Some(state), error, code) {
        (false, _, _) => (
            "404 Not Found",
            "Not the sign-in mockingbird-studio started.",
            None,
        ),
        (true, Some(e), _) => (
            "200 OK",
            "Not signed in. You can close this tab.",
            Some(Err(format!("Google didn't sign you in: {e}"))),
        ),
        (true, None, Some(c)) => (
            "200 OK",
            "Signed in. You can close this tab and go back to mockingbird-studio.",
            Some(Ok(c)),
        ),
        (true, None, None) => ("400 Bad Request", "No code came back.", None),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    result
}

/// `youtube_connect`: signs in through the browser and keeps the refresh
/// token in the Keychain under `label`.
pub fn connect(label: &str, client_file: &str) -> Result<Value> {
    check_label(label)?;
    let (client_id, client_secret) = read_client(client_file)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("can't listen: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = random(32)?;
    let state = random(16)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", client_id.as_str()),
            ("redirect_uri", &redirect),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            // A refresh token comes back only with these two.
            ("access_type", "offline"),
            ("prompt", "consent"),
        ])
        .finish();
    open_browser(&format!("{}?{query}", endpoint(AUTH)))?;
    let code = wait_for_code(&listener, &state)?;
    let tokens = post(
        TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("redirect_uri", &redirect),
            ("code_verifier", &verifier),
        ],
    )?;
    let granted = tokens["scope"].as_str().unwrap_or_default();
    if !granted.split(' ').any(|s| s == SCOPE) {
        return Err("Google didn't grant the upload permission. Sign in again and allow it".into());
    }
    let refresh_token = tokens["refresh_token"]
        .as_str()
        .ok_or("Google sent no refresh token")?
        .to_string();
    save(
        label,
        &Secret {
            client_id,
            client_secret,
            refresh_token,
        },
    )?;
    store::update_config(|c| {
        if !c.channel.iter().any(|ch| ch.label == label) {
            c.channel.push(Channel {
                label: label.to_string(),
                ..Channel::default()
            });
        }
        Ok(())
    })?;
    Ok(json!({"label": label, "signed_in": true, "scope": SCOPE}))
}

/// `youtube_status`: each channel's label, whether it has a token, and its id
/// and title once an upload has told the app.
pub fn status() -> Result<Value> {
    let config = store::read_config()?;
    let channels = config
        .channel
        .iter()
        .map(|c| {
            Ok(json!({
                "label": c.label,
                "signed_in": load(&c.label)?.is_some(),
                "id": c.id,
                "title": c.title,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"channels": channels}))
}

/// `youtube_disconnect`: revokes the token at Google and deletes it. It's
/// deleted even when Google can't be reached; the reply says so.
pub fn disconnect(label: &str) -> Result<Value> {
    check_label(label)?;
    let secret = load(label)?.ok_or_else(|| format!("{label} isn't signed in"))?;
    let revoked = post(REVOKE, &[("token", &secret.refresh_token)]);
    delete(label)?;
    let mut out = json!({"label": label, "signed_in": false, "revoked": revoked.is_ok()});
    if let Err(e) = revoked {
        out["note"] = format!(
            "{e}. The token is deleted here; remove the app's access at myaccount.google.com/permissions"
        )
        .into();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_plain_names() {
        assert!(check_label("main").is_ok());
        assert!(check_label("ai-news_2").is_ok());
        for bad in ["", "../x", "a b", "a/b"] {
            assert!(check_label(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_web_client_file_is_refused_without_quoting_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client.json");
        fs::write(
            &path,
            r#"{"web":{"client_id":"id","client_secret":"hush"}}"#,
        )
        .unwrap();
        let e = read_client(path.to_str().unwrap()).unwrap_err();
        assert!(e.contains("Desktop app") && !e.contains("hush"), "{e}");
    }

    /// Writes, reads and deletes a real Keychain item.
    #[test]
    #[ignore = "touches the login Keychain"]
    fn the_keychain_round_trip() {
        let label = "studio-test-keychain";
        let s = Secret {
            client_id: "id".into(),
            client_secret: "secret".into(),
            refresh_token: "refresh".into(),
        };
        save(label, &s).unwrap();
        assert_eq!(load(label).unwrap().unwrap().refresh_token, "refresh");
        delete(label).unwrap();
        assert!(load(label).unwrap().is_none());
    }
}
