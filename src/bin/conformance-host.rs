use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    time::Instant,
};
use tauri_plugin_jelto::{InstallOrigin, Jelto, Props};

// Only command parsing and result encoding live here. Timers, state, validation,
// network delivery and the virtual clock are the shipping engine's implementation.
fn tokens(line: &str) -> Vec<String> {
    let mut rest = line.trim();
    let mut result = Vec::new();
    while !rest.is_empty() {
        if rest.starts_with(['{', '[']) {
            result.push(rest.to_owned());
            break;
        }
        if let Some(quoted) = rest.strip_prefix('"') {
            let mut token = String::new();
            let mut chars = quoted.char_indices();
            let mut consumed = quoted.len();
            while let Some((i, c)) = chars.next() {
                if c == '"' {
                    consumed = i + 1;
                    break;
                }
                if c == '\\' {
                    if let Some((_, next)) = chars.next() {
                        token.push(next);
                    }
                } else {
                    token.push(c);
                }
            }
            result.push(token);
            rest = quoted[consumed..].trim_start();
        } else {
            let end = rest.find([' ', '\t']).unwrap_or(rest.len());
            result.push(rest[..end].to_owned());
            rest = rest[end..].trim_start();
        }
    }
    result
}

async fn dispatch(sdk: &Jelto, args: &[String]) -> Result<Value, &'static str> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let arg = |i: usize| args.get(i).map(String::as_str);
    let start = Instant::now();
    let mut reply = json!({"cmd":cmd,"ok":true});
    match cmd {
        "init" => {
            sdk.init_with_origin(
                arg(1).ok_or("init <key> [app]")?,
                arg(2),
                None,
                InstallOrigin::from(
                    std::env::var("JELTO_INSTALL_ORIGIN")
                        .unwrap_or_default()
                        .as_str(),
                ),
            )
            .await
        }
        "track" => {
            let props = arg(2)
                .map(serde_json::from_str::<Props>)
                .transpose()
                .map_err(|_| "invalid JSON props")?;
            sdk.track(arg(1).ok_or("track <name> [json-props]")?, props)
                .await;
        }
        "onboarding" => {
            sdk.onboarding(
                arg(1).ok_or("onboarding <step> <status> [reason]")?,
                arg(2).ok_or("missing status")?,
                arg(3),
            )
            .await
        }
        "setprops" => {
            sdk.set_props(
                serde_json::from_str(arg(1).ok_or("setprops <json>")?)
                    .map_err(|_| "invalid JSON props")?,
            )
            .await
        }
        "installid" => reply["value"] = json!(sdk.install_id().await),
        "dumpstate" => reply["state"] = sdk.export_state().await,
        "legacyversion" => {
            if !sdk.legacy_version().await {
                return Err(
                    "legacyversion requires initialized durable state without a pending transition",
                );
            }
        }
        "reset" => sdk.reset().await,
        "disable" => sdk.disable().await,
        "sleep" => {
            sdk.advance(
                arg(1)
                    .ok_or("sleep <ms>")?
                    .parse()
                    .map_err(|_| "sleep requires whole milliseconds >= 0")?,
            )
            .await
        }
        "exit" => sdk.stop().await,
        _ => return Err("unknown command"),
    }
    if matches!(cmd, "init" | "track") {
        reply["us"] = json!(start.elapsed().as_micros() as u64);
    }
    Ok(reply)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let sdk = match Jelto::conformance() {
        Ok(sdk) => sdk,
        Err(error) => {
            eprintln!("jelto: {error}");
            std::process::exit(2);
        }
    };
    // Blocking stdin belongs only to this headless driver; the SDK has its own worker.
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let args = tokens(&line);
        let command = args.first().map(String::as_str).unwrap_or("");
        let reply = match dispatch(&sdk, &args).await {
            Ok(reply) => reply,
            Err(error) => json!({"cmd":command,"ok":false,"error":error}),
        };
        if writeln!(stdout, "{reply}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            break;
        }
        if command == "exit" {
            return;
        }
    }
    sdk.stop().await;
}

#[test]
fn quoted_tokens_and_property_literals() {
    assert_eq!(
        tokens("onboarding \"Bad Step!\" ok \"Free text\""),
        ["onboarding", "Bad Step!", "ok", "Free text"]
    );
    assert_eq!(
        tokens("track x {\"x\": 9007199254740993}"),
        ["track", "x", "{\"x\": 9007199254740993}"]
    );
}
