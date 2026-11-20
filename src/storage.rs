//! Settings → Storage: the places games download and install to, each with how much room it
//! has; new ones can be added (an SSD, a second drive), and any becomes where new games go.
//!
//! A place added by its folder P keeps SwarmPlay's things in P/SwarmPlay/Downloads and
//! P/SwarmPlay/Games (the latter is also a game folder, so what's installed there is found).

use crate::app::*;
use crate::settings::SId;
use crate::{util, SettingData};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// One place: where its downloads and installs go, and how much room its drive has.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub download: String,
    pub install: String,
    pub free: u64,
    pub total: u64,
    pub default: bool,
    dev: u64,
}

/// The nearest folder of `path` that exists.
fn existing(path: &Path) -> Option<&Path> {
    path.ancestors().find(|p| p.is_dir())
}

/// Free and total bytes on the drive holding `path`.
fn space(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    let dir = existing(path)?;
    let c = CString::new(dir.as_os_str().as_encoded_bytes()).ok()?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) } != 0 { return None; }
    let st = unsafe { st.assume_init() };
    let unit = st.f_frsize as u64;
    Some((st.f_bavail as u64 * unit, st.f_blocks as u64 * unit))
}

/// Where the drive holding `path` is mounted (the longest mount point above it).
fn mount_point(path: &Path) -> String {
    let path = existing(path).and_then(|p| p.canonicalize().ok()).unwrap_or_else(|| path.to_path_buf());
    let mounts = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    mounts.lines().filter_map(|l| l.split(' ').nth(4)).map(|m| m.replace("\\040", " "))
        .filter(|m| path.starts_with(m)).max_by_key(|m| m.len()).unwrap_or_else(|| "/".into())
}

/// A drive's name for people: the system's own drive, or the removable one's label.
pub fn drive_name(mount: &str) -> String {
    let internal = ["/", "/var", "/home", "/var/home", "/sysroot", "/usr"];
    if internal.contains(&mount) { return "Internal storage".into(); }
    Path::new(mount).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| mount.to_string())
}

/// A place added by its folder: SwarmPlay's downloads and games inside it.
fn under(folder: &str) -> (String, String) {
    let base = Path::new(folder.trim_end_matches('/')).join("SwarmPlay");
    (base.join("Downloads").to_string_lossy().into_owned(), base.join("Games").to_string_lossy().into_owned())
}

fn gb(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if g >= 1024.0 { format!("{:.1} TB", g / 1024.0) } else { format!("{g:.0} GB") }
}

impl App {
    /// Every place, the one new games go to first; one per drive.
    pub fn storage_places(&self) -> Vec<Place> {
        let cfg = self.cfg.lock().unwrap().clone();
        let mut pairs = vec![(cfg.download_dir.clone(), cfg.install_dir.clone())];
        pairs.push(("~/Downloads/Azrak".into(), "~/Games/Azrak".into()));
        pairs.extend(cfg.storage.iter().map(|f| under(f)));
        let mut places: Vec<Place> = Vec::new();
        for (download, install) in pairs {
            let path = util::expand_home(&install);
            let dev = existing(&path).and_then(|p| std::fs::metadata(p).ok()).map(|m| m.dev()).unwrap_or(0);
            if places.iter().any(|p| p.dev == dev) { continue; }
            let (free, total) = space(&path).unwrap_or((0, 0));
            let default = install == cfg.install_dir;
            places.push(Place { name: drive_name(&mount_point(&path)), download, install, free, total, default, dev });
        }
        places
    }

    pub fn storage_rows(&self) -> (Vec<(SId, SettingData)>, Vec<Place>) {
        let cfg = self.cfg.lock().unwrap().clone();
        let row = |kind: i32, label: &str| SettingData { kind, label: label.into(), ..Default::default() };
        let places = self.storage_places();
        let mut rows = Vec::new();
        for (i, p) in places.iter().enumerate() {
            let games = self.locals.iter().filter(|l| existing(&l.l.path).and_then(|d| std::fs::metadata(d).ok()).is_some_and(|m| m.dev() == p.dev)).count();
            let mut r = row(4, &p.name);
            r.value = if p.total > 0 { format!("{} free of {}", gb(p.free), gb(p.total)).into() } else { "Not connected".into() };
            r.value_kind = if p.total > 0 && p.free < 50 << 30 { 2 } else { 0 };
            r.has_bar = p.total > 0;
            r.bar = if p.total > 0 { 1.0 - p.free as f32 / p.total as f32 } else { 0.0 };
            let what = if games == 1 { "1 game".to_string() } else { format!("{games} games") };
            (r.hint, r.hint_kind) = if p.default {
                (format!("New games go here · {what} · {}", util::display_path(&p.install)).into(), 1)
            } else {
                (format!("{what} · press to put new games here · {}", util::display_path(&p.install)).into(), 0)
            };
            rows.push((SId::StoragePlace(i as u16), r));
        }
        let mut r = row(1, "Add a place");
        r.hint = "A folder on another drive (an SSD, a USB drive): games can download and install there".into();
        rows.push((SId::StorageAdd, r));
        let mut r = row(1, "Install new games to");
        r.value = util::display_path(&cfg.install_dir).into();
        r.hint = "Downloaded games are set up here when you press Install".into();
        rows.push((SId::InstallDir, r));
        let mut r = row(1, "Save downloads to");
        r.value = util::display_path(&cfg.download_dir).into();
        r.hint = "Each download gets its own folder".into();
        rows.push((SId::DownloadDir, r));
        (rows, places)
    }

    /// Press on a place: new games go there from now on.
    pub fn storage_use(&mut self, i: usize) {
        let Some(place) = self.storage_places().get(i).cloned() else { return };
        if place.default {
            return self.note_setting(SId::StoragePlace(i as u16), "New games already go here", 0);
        }
        for dir in [&place.download, &place.install] {
            if let Err(e) = std::fs::create_dir_all(util::expand_home(dir)) {
                return self.note_setting(SId::StoragePlace(i as u16), &format!("Can't use it: {e}"), 2);
            }
        }
        {
            let mut c = self.cfg.lock().unwrap();
            c.download_dir = place.download.clone();
            c.install_dir = place.install.clone();
            if !c.game_dir_paths().iter().any(|d| *d == util::expand_home(&place.install)) { c.game_dirs.push(place.install.clone()); }
            c.save();
        }
        crate::audio::play(crate::audio::Sound::Select);
        self.toast("New games go here now", &place.name, "disk", crate::toast::DONE);
        self.refresh_settings();
    }

    /// "Add a place": the folder typed.
    pub fn storage_add(&mut self, text: &str) {
        let folder = text.trim().trim_end_matches('/').to_string();
        let path: PathBuf = util::expand_home(&folder);
        if folder.is_empty() { return; }
        if !path.is_dir() {
            return self.note_setting(SId::StorageAdd, "Not added: no folder there (is the drive connected?)", 2);
        }
        let (download, install) = under(&folder);
        if let Err(e) = std::fs::create_dir_all(util::expand_home(&install)).and_then(|_| std::fs::create_dir_all(util::expand_home(&download))) {
            return self.note_setting(SId::StorageAdd, &format!("Not added: can't write there ({e})"), 2);
        }
        {
            let mut c = self.cfg.lock().unwrap();
            if !c.storage.contains(&folder) { c.storage.push(folder); }
            if !c.game_dir_paths().iter().any(|d| *d == util::expand_home(&install)) { c.game_dirs.push(install); }
            c.save();
        }
        self.note_setting(SId::StorageAdd, "Added · press it in the list to put new games there", 1);
        self.refresh_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_drives_and_places_folders_under_swarmplay() {
        assert_eq!(drive_name("/"), "Internal storage");
        assert_eq!(drive_name("/var/home"), "Internal storage");
        assert_eq!(drive_name("/run/media/m/Games SSD"), "Games SSD");
        assert_eq!(under("/run/media/m/SSD/"), ("/run/media/m/SSD/SwarmPlay/Downloads".into(), "/run/media/m/SSD/SwarmPlay/Games".into()));
    }
}
