// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Open a link in the person's own browser: the Solo DJ's microphone link (ADR-0274), which
//! joins the room from a browser tab beside this app (the app does not join the room itself,
//! ADR-0013). The window's webview opens nothing outside itself, so the page asks here.
//!
//! Only `http` and `https` addresses with a host are handed on, as one argument to the
//! platform's own opener and never through a shell, so nothing the page sends can become a
//! command or open a file.

use std::process::{Command, Stdio};

/// The address as it is handed to the opener, or why it is not: a web address with a host.
pub fn web_address(link: &str) -> Result<String, String> {
    let url = url::Url::parse(link.trim()).map_err(|_| "that is not a web address".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return Err("only http and https addresses are opened".to_owned());
    }
    Ok(url.into())
}

/// The platform's opener for a web address.
fn opener(address: &str) -> Command {
    #[cfg(target_os = "windows")]
    let command = {
        // Not `cmd /C start`: a shell would read `&` in an address as its own.
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", address]);
        c
    };
    #[cfg(target_os = "macos")]
    let command = {
        let mut c = Command::new("open");
        c.arg(address);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let command = {
        let mut c = Command::new("xdg-open");
        c.arg(address);
        c
    };
    command
}

/// Open a web address in the person's browser. An error is a sentence for the page, which then
/// offers the link to copy instead.
#[tauri::command]
pub fn open_link(url: String) -> Result<(), String> {
    let address = web_address(&url)?;
    let mut child = opener(&address)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("no browser could be opened: {e}"))?;
    // The opener hands the address on and exits; wait for it off this thread, so it is reaped.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::web_address;

    #[test]
    fn a_join_link_goes_through_whole_with_its_pass_in_the_fragment() {
        assert_eq!(
            web_address(" https://kettle.example/join/#rsp1.abc-DEF_123 ").as_deref(),
            Ok("https://kettle.example/join/#rsp1.abc-DEF_123")
        );
        assert_eq!(
            web_address("http://127.0.0.1:17400/join/#p").as_deref(),
            Ok("http://127.0.0.1:17400/join/#p")
        );
    }

    #[test]
    fn nothing_but_a_web_address_is_opened() {
        for link in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "/join/#pass",
            "kettle.example/join/",
            "https://",
            "",
            "--version",
        ] {
            assert!(web_address(link).is_err(), "{link}");
        }
    }

    #[test]
    fn an_address_is_one_argument_whatever_it_holds() {
        // Spaces and shell characters are percent-encoded or kept inside the one argument.
        let a = web_address("https://kettle.example/join/?a=1&b=2 3#x;y").unwrap();
        assert!(a.starts_with("https://kettle.example/join/?a=1&b=2%203#"));
        assert!(!a.contains(' '));
    }
}
