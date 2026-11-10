pub mod apps;
pub mod buses;
pub mod channels;
pub mod devices;
pub mod eq;
pub mod hotkeys;
pub mod mic;
pub mod profiles;
pub mod routing;
pub mod settings;
pub mod updates;

/// Open a fixed, code-chosen URL in the user's browser. The URL never comes
/// from the frontend, so a compromised webview can't open arbitrary pages.
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            // Reap it off-thread: some desktops' openers block until the
            // browser exits, and an unwaited child lingers as a zombie.
            std::thread::spawn(move || child.wait());
        })
        .map_err(|e| format!("open {url}: {e}"))
}
