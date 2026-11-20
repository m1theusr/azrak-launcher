//! Controllers in Windows games. Wine hands Sony controllers (DualShock 4, DualSense) to games
//! raw, through hidraw: a game that only knows XInput (most PC games, RE Engine's among them)
//! then sees no controller at all. Through SDL instead, Wine shows every controller as an XInput
//! one. Before a Windows game starts, its Wine prefix is set that way (Settings → Playing →
//! Controller in Windows games); Proton gets the same through PROTON_PREFER_SDL.

use std::path::{Path, PathBuf};

/// The winebus settings in a prefix's system.reg (HKLM\System\CurrentControlSet links here).
const SECTION: &str = r"[System\\ControlSet001\\Services\\winebus]";
const VALUES: [(&str, &str); 2] = [("\"DisableHidraw\"", "dword:00000001"), ("\"Enable SDL\"", "dword:00000001")];

/// Wine prefixes in a game's folder (a repack's game/prefix, say): folders with a system.reg
/// and a drive_c, a few levels down at most.
pub fn prefixes_in(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        if dir.join("system.reg").is_file() && dir.join("drive_c").is_dir() {
            out.push(dir.to_path_buf());
            return;
        }
        if depth == 0 { return; }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            // Not into the game's own files (drive_c of another prefix, big data folders).
            if p.is_dir() && !e.file_type().is_ok_and(|t| t.is_symlink()) { walk(&p, depth - 1, out); }
        }
    }
    let mut out = Vec::new();
    walk(dir, 3, &mut out);
    out
}

/// `text` (a system.reg) with controllers going through SDL; None when it already does.
fn with_sdl(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|l| l.starts_with(SECTION)) else {
        // No winebus key yet: add it (Wine reads it at the next start).
        let mut out = text.trim_end().to_string();
        out.push_str(&format!("\n\n{SECTION} 0\n"));
        for (k, v) in VALUES { out.push_str(&format!("{k}={v}\n")); }
        return Some(out);
    };
    let end = lines[start + 1..].iter().position(|l| l.trim().is_empty() || l.starts_with('[')).map_or(lines.len(), |n| start + 1 + n);
    let mut section: Vec<String> = lines[start + 1..end].iter().map(|l| l.to_string()).collect();
    let mut changed = false;
    for (k, v) in VALUES {
        let want = format!("{k}={v}");
        match section.iter().position(|l| l.starts_with(&format!("{k}="))) {
            Some(i) if section[i] == want => {}
            Some(i) => { section[i] = want; changed = true; }
            None => { section.push(want); changed = true; }
        }
    }
    if !changed { return None; }
    let mut out: Vec<String> = lines[..=start].iter().map(|l| l.to_string()).collect();
    out.extend(section);
    out.extend(lines[end..].iter().map(|l| l.to_string()));
    Some(out.join("\n") + "\n")
}

/// Set every prefix under `dir` (or `dir` itself) to hand controllers through SDL. Only while
/// the game isn't running (Wine keeps the registry in memory and writes it back on exit).
pub fn prefer_sdl(dir: &Path) {
    for prefix in prefixes_in(dir) {
        let reg = prefix.join("system.reg");
        let Ok(text) = std::fs::read_to_string(&reg) else { continue };
        if let Some(new) = with_sdl(&text) {
            let backup = prefix.join("system.reg.swarmplay-backup");
            if !backup.exists() { let _ = std::fs::copy(&reg, &backup); }
            match crate::util::atomic_write(&reg, new.as_bytes()) {
                Ok(()) => crate::log!("controllers through SDL in {}", prefix.display()),
                Err(e) => crate::log!("couldn't set controllers through SDL in {}: {e}", prefix.display()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_hidraw_off_and_sdl_on_in_the_winebus_key() {
        let reg = "WINE REGISTRY Version 2\n\n[System\\\\ControlSet001\\\\Services\\\\winebus] 1794966133\n#time=1dd\n\"Description\"=\"Wine HID bus driver\"\n\"Enable SDL\"=dword:00000000\n\n[Software] 1\n\"x\"=\"y\"\n";
        let out = with_sdl(reg).unwrap();
        assert!(out.contains("\"Enable SDL\"=dword:00000001"));
        assert!(out.contains("\"DisableHidraw\"=dword:00000001"));
        assert!(!out.contains("\"Enable SDL\"=dword:00000000"));
        assert!(out.contains("[Software] 1\n\"x\"=\"y\""), "the rest stays as it was");
        assert_eq!(with_sdl(&out), None, "already set: left alone");
    }

    #[test]
    fn adds_the_key_when_the_prefix_has_none() {
        let out = with_sdl("WINE REGISTRY Version 2\n").unwrap();
        assert!(out.contains("[System\\\\ControlSet001\\\\Services\\\\winebus] 0\n\"DisableHidraw\"=dword:00000001\n\"Enable SDL\"=dword:00000001"));
    }
}
