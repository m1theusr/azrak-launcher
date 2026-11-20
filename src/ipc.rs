//! Commands to the SwarmPlay that's running, from `swarmplay --overlay` (and --home,
//! --screenshot): a desktop keyboard shortcut runs one of those, since a program on Wayland can't
//! catch keys while a game has focus. A Unix socket in the runtime folder.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub const COMMANDS: [&str; 3] = ["overlay", "home", "screenshot"];

fn socket() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("swarmplay.sock")
}

/// Hand `command` to the running SwarmPlay. False: none is running.
pub fn send(command: &str) -> bool {
    UnixStream::connect(socket()).and_then(|mut s| s.write_all(command.as_bytes())).is_ok()
}

/// Listen for commands; `handle` gets each one (on this thread: it posts to the UI).
pub fn listen(handle: impl Fn(String) + Send + 'static) {
    let path = socket();
    // Another SwarmPlay answers there: leave it be.
    if UnixStream::connect(&path).is_ok() { return; }
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return };
    std::thread::Builder::new().name("ipc".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut text = String::new();
            let _ = stream.take(64).read_to_string(&mut text);
            let command = text.trim().to_string();
            if COMMANDS.contains(&command.as_str()) { handle(command); }
        }
    }).ok();
}
