//! The in-game overlay, as on a console: with a game in front, the PS button brings a panel up
//! over it (ui/overlay.slint, a second window), and a tap or ○ gives the game back. Holding PS
//! goes to SwarmPlay's Home; Share takes a screenshot. Notifications show over the game too,
//! without taking its input. How the window gets above the game depends on the desktop
//! (src/overlay.rs); where it can't, PS brings the launcher up instead.

use crate::app::*;
use crate::audio::{self, Sound};
use crate::overlay::{self, Desktop, Helper};
use crate::settings::SId;
use crate::{OverlayWindow, SettingData, ToastData};
use slint::ComponentHandle;
use slint::winit_030::{winit, WinitWindowAccessor};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Items of the panel: the top row, then the bottom row (ui/overlay.slint's order).
const TOP: usize = 4;
const ITEMS: usize = 8;

impl App {
    /// A game is in front: one runs, and its window is the focused one (what X11 and Gamescope
    /// say; under Gamescope the launcher's own window never learns it lost focus) or the
    /// launcher's window isn't (Wayland, where nothing else can tell).
    pub fn in_game(&self) -> bool {
        !self.live.is_empty() && (self.sessions.game_in_front() || !crate::display::window_has_focus(&self.ui()))
    }

    /// Whether the overlay can show here, in the background (D-Bus at most); Settings follows.
    pub fn ov_check_helper(&mut self) {
        std::thread::spawn(|| {
            let h = overlay::helper();
            let _ = slint::invoke_from_event_loop(move || with_app(move |app| {
                if app.ov_helper != h {
                    app.ov_helper = h;
                    if app.overlay == Overlay::Settings && app.edit_index < 0 { app.refresh_settings(); }
                }
            }));
        });
    }

    /// The overlay window, made the first time it's needed.
    fn ov_window(&mut self) -> OverlayWindow {
        if let Some(w) = &self.ov { return w.clone_strong(); }
        let w = OverlayWindow::new().expect("overlay window");
        w.on_click(|i| with_app(move |app| app.ov_click(i)));
        w.on_key(|text| with_app(move |app| app.ov_key(&text)));
        self.ov = Some(w.clone_strong());
        self.ov_style();
        w
    }

    /// The overlay's own copy of the scale and the theme (each window has its own).
    pub fn ov_style(&self) {
        let Some(w) = &self.ov else { return };
        let main = self.ui();
        let (s, t) = (main.global::<crate::S>(), main.global::<crate::Theme>());
        w.global::<crate::S>().set_k(s.get_k());
        w.global::<crate::S>().set_t(s.get_t());
        w.global::<crate::Theme>().set_accent(t.get_accent());
        w.global::<crate::Theme>().set_glow(t.get_glow());
    }

    /// PS tapped (or `swarmplay --overlay`, `keyboard`): the panel over the game, or away again.
    /// From the controller the game keeps the focus (the controller reaches SwarmPlay anyway, and
    /// many full-screen games stop drawing, or minimise, once they lose it); from the keyboard the
    /// panel takes it, for the keys.
    pub fn ov_ps(&mut self, keyboard: bool) {
        crate::log!("overlay: PS · open {} · in game {} · {:?} · {:?}", self.ov_open, self.in_game(), overlay::desktop(), self.ov_helper);
        if self.ov_open { return self.ov_close(true); }
        if !self.in_game() { return self.cc_from_pad(); }
        match (overlay::desktop(), self.ov_helper) {
            // Steam's Game Mode: the PS button is Steam's.
            (Desktop::SteamGameMode, _) => {}
            (_, Helper::Ready) => self.ov_show(keyboard),
            // No way above the game here: the launcher comes up, its Control Center open.
            (_, h) => {
                if matches!(h, Helper::Missing | Helper::NeedsLogin) {
                    let _ = std::process::Command::new("notify-send").args(["SwarmPlay",
                        if h == Helper::Missing { "For the in-game overlay, install the GNOME helper: Settings → Playing → In-game overlay" }
                        else { "Log out and in once to finish setting up the in-game overlay" }]).spawn();
                }
                std::thread::spawn(|| { overlay::focus_main(); });
                if self.overlay != Overlay::ControlCenter { self.toggle_control_center(); }
            }
        }
    }

    /// PS outside a game: the Control Center, over whatever the launcher shows.
    fn cc_from_pad(&mut self) {
        // A notification on screen: PS opens the list on it, as on a console.
        if self.overlay == Overlay::None {
            if let Some(id) = self.popup_showing() { return self.open_notifications(Some(id)); }
        }
        if !crate::display::window_has_focus(&self.ui()) {
            std::thread::spawn(|| { overlay::focus_main(); });
        }
        self.toggle_control_center();
    }

    /// PS held (or `swarmplay --home`): SwarmPlay's Home, the game going on behind it.
    pub fn ov_home(&mut self) {
        if overlay::desktop() == Desktop::SteamGameMode && self.in_game() { return; }
        if self.ov_open { self.ov_close(false); }
        std::thread::spawn(|| { overlay::focus_main(); });
        if self.overlay == Overlay::ControlCenter { self.back(); }
        if self.view != 0 { self.switch_view(0, true); }
        audio::play(Sound::Select);
    }

    fn ov_show(&mut self, focus: bool) {
        let w = self.ov_window();
        self.ov_style();
        self.ov_open = true;
        self.ov_idx = 0;
        self.ov_confirm = false;
        self.ov_vol_edit = false;
        self.ov_game = self.live.first().map(|s| s.pid);
        self.ov_push();
        w.set_open(false);
        let _ = w.show();
        Self::ov_fullscreen(&w);
        w.window().with_winit_window(|win: &winit::window::Window| {
            let _ = win.set_cursor_hittest(true);
            // X11 window managers keep it on top when asked (Wayland ones ignore this: helpers).
            win.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        });
        // In next frame, so the panel slides up from below.
        let w2 = w.as_weak();
        slint::Timer::single_shot(Duration::from_millis(30), move || { if let Some(w) = w2.upgrade() { w.set_open(true); } });
        crate::gamepad::GRAB.store(true, Ordering::Relaxed);
        if self.cfg.lock().unwrap().overlay_pause {
            if let Some(pid) = self.ov_game { unsafe { libc::killpg(pid as i32, libc::SIGSTOP) }; self.ov_paused = Some(pid); }
        }
        // Over the game: the same sounds, a little quieter.
        audio::set_over_game(true);
        audio::play(Sound::Open);
        std::thread::spawn(move || {
            // Mapped first: the desktop finds it by its title.
            std::thread::sleep(Duration::from_millis(60));
            let raised = overlay::raise_overlay(focus);
            crate::log!("overlay: shown · above the game: {raised} · focus: {focus}");
        });
    }

    /// The panel down; `to_game` gives the game its focus back.
    pub fn ov_close(&mut self, to_game: bool) {
        if !self.ov_open { return; }
        self.ov_open = false;
        audio::set_over_game(false);
        crate::gamepad::GRAB.store(false, Ordering::Relaxed);
        if let Some(pid) = self.ov_paused.take() { unsafe { libc::killpg(pid as i32, libc::SIGCONT) }; }
        audio::play(Sound::Back);
        if let Some(w) = &self.ov { w.set_open(false); }
        let game = self.ov_game;
        slint::Timer::single_shot(Duration::from_millis(380), move || with_app(move |app| {
            // Gone once it has slid away (unless it opened again, or a notification shows).
            if !app.ov_open && !app.ov.as_ref().is_some_and(|w| w.get_toast_shown()) {
                if let Some(w) = &app.ov { let _ = w.hide(); }
            }
        }));
        if to_game {
            if let Some(pid) = game { std::thread::spawn(move || { overlay::focus_game(pid); }); }
        }
    }

    /// Full screen once shown, and again a moment later: asked while it's hidden or still being
    /// mapped, X11 window managers drop it, and Slint skips asking again for the state it thinks
    /// the window has. (On GNOME and KDE the helpers make it full screen themselves.)
    fn ov_fullscreen(w: &OverlayWindow) {
        for ms in [0u64, 80, 300] {
            let w = w.as_weak();
            slint::Timer::single_shot(Duration::from_millis(ms), move || {
                if let Some(w) = w.upgrade() {
                    w.window().set_fullscreen(false);
                    w.window().set_fullscreen(true);
                }
            });
        }
    }

    /// What the panel shows, as it is now.
    pub fn ov_push(&mut self) {
        let Some(w) = self.ov.as_ref().map(|w| w.clone_strong()) else { return };
        let game = self.live.first();
        w.set_game_name(game.map(|s| s.name.clone()).unwrap_or_default().into());
        w.set_game_time(game.map(|s| crate::util::fmt_duration_words(crate::util::now_secs() - s.since)).unwrap_or_default().into());
        w.set_idx(self.ov_idx);
        w.set_confirm_close(self.ov_confirm);
        w.set_vol_edit(self.ov_vol_edit);
        w.set_pad_hints(self.pad_hints);
        w.set_clock(self.clock.clone().into());
        let jobs = self.downloads.snapshot();
        let active: Vec<_> = jobs.iter().filter(|j| j.state.active()).collect();
        let (title, sub, progress) = match active.first() {
            Some(j) => {
                let p = if j.total > 0 { j.done as f32 / j.total as f32 } else { 0.0 };
                (j.name.clone(), format!("{:.0}%{}", p * 100.0, if active.len() > 1 { format!(" · {} more", active.len() - 1) } else { String::new() }), p)
            }
            None => ("Nothing downloading".to_string(), "Open Downloads".to_string(), -1.0),
        };
        w.set_dl_title(title.into());
        w.set_dl_sub(sub.into());
        w.set_dl_progress(progress);
        w.set_capture_sub(match &self.last_capture {
            Some(name) => format!("Last: {name}").into(),
            None => "Saved in Pictures → SwarmPlay".into(),
        });
        let v = self.sound.volume;
        w.set_volume(match v { Some(_) if self.sound.muted => "Muted".into(), Some(v) => format!("{v}%").into(), None => "Sound".into() });
        w.set_volume_level(if self.sound.muted { 0.0 } else { v.map_or(0.0, |v| v as f32 / 100.0) });
        w.set_pad_battery(crate::battery::controllers().first().map(|b| format!("{}%", b.percent)).unwrap_or_default().into());
    }

    /// The controller (or the keyboard) while the panel is up.
    pub fn ov_act(&mut self, a: Act) {
        let i = self.ov_idx.clamp(0, ITEMS as i32 - 1) as usize;
        // Sound, after ✕: ◂ ▸ turn the volume down and up; ✕, ○ (or moving up) are done with it.
        if self.ov_vol_edit {
            match a {
                Act::Left | Act::Right => { self.sound_change(crate::settings::SId::SoundVolume, if a == Act::Left { -1 } else { 1 }); }
                Act::Confirm | Act::Back | Act::Up | Act::Down => { self.ov_vol_edit = false; audio::play(Sound::Back); }
                _ => {}
            }
            return self.ov_push();
        }
        if let Some(to) = crate::control_center::step(ITEMS, TOP, i, a) {
            self.ov_idx = to as i32;
            self.ov_confirm = false;
            audio::play(Sound::Move);
            return self.ov_push();
        }
        match a {
            Act::Back => self.ov_close(true),
            Act::Confirm => self.ov_choose(i),
            _ => {}
        }
    }

    fn ov_click(&mut self, i: i32) {
        if i < 0 { return self.ov_close(true); }
        self.ov_idx = i;
        self.ov_choose(i as usize);
    }

    fn ov_key(&mut self, text: &str) {
        use slint::platform::Key;
        let k = |key: Key| slint::SharedString::from(key) == text;
        let a = if k(Key::UpArrow) { Act::Up } else if k(Key::DownArrow) { Act::Down } else if k(Key::LeftArrow) { Act::Left }
            else if k(Key::RightArrow) { Act::Right } else if k(Key::Return) || text == " " { Act::Confirm }
            else if k(Key::Escape) || k(Key::Backspace) { Act::Back } else { return };
        if self.pad_hints { self.pad_hints = false; }
        self.ov_act(a);
    }

    fn ov_choose(&mut self, i: usize) {
        match i {
            0 => self.ov_close(true),
            1 if !self.ov_confirm => { self.ov_confirm = true; audio::play(Sound::Move); self.ov_push(); }
            1 => { self.ov_close(false); self.stop_game(None); std::thread::spawn(|| { overlay::focus_main(); }); }
            2 => { self.ov_home(); let key = self.ring_key.clone(); self.open_downloads(key.as_deref()); }
            3 => self.ov_capture(),
            4 => self.ov_home(),
            5 => { self.ov_vol_edit = true; audio::play(Sound::Select); self.ov_push(); }
            6 => { self.ov_home(); self.open_settings(); }
            _ => { self.ov_home(); self.open_power_menu(); }
        }
    }

    /// Share (or the Capture card, or `swarmplay --screenshot`): the game's picture, saved under
    /// Pictures/SwarmPlay/<game>/, the panel out of the way first.
    pub fn ov_capture(&mut self) {
        let game = self.live.first().map(|s| s.name.clone()).unwrap_or_else(|| "SwarmPlay".into());
        let was_open = self.ov_open;
        if was_open { self.ov_close(true); }
        let folder: String = game.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
        let tm = crate::util::local_time();
        let name = format!("{:04}-{:02}-{:02} {:02}.{:02}.{:02}.png", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec);
        let pictures = std::env::var_os("XDG_PICTURES_DIR").map(std::path::PathBuf::from)
            .unwrap_or_else(|| crate::util::expand_home("~/Pictures"));
        let path = pictures.join("SwarmPlay").join(folder.trim()).join(&name);
        std::thread::spawn(move || {
            // The panel's slide away, before the picture.
            std::thread::sleep(Duration::from_millis(if was_open { 450 } else { 50 }));
            let result = overlay::screenshot(&path);
            let path_s = path.to_string_lossy().into_owned();
            let _ = slint::invoke_from_event_loop(move || with_app(move |app| match result {
                Ok(()) => {
                    app.last_capture = Some(name.trim_end_matches(".png").to_string());
                    audio::play(Sound::Shutter);
                    // The answer to the player's own press: shown even in a game, as on a console.
                    app.notify(crate::toast::Cat::Games, crate::toast::DONE, "check", "Screenshot saved", &format!("Pictures → SwarmPlay → {}", folder.trim()),
                        crate::toast::Action::File(path_s.clone()), "", true);
                }
                Err(e) => app.toast("No screenshot", &e, "alert", crate::toast::FAILED),
            }));
        });
    }

    /// A notification while a game is in front: over the game, without taking its input.
    /// False when it can't show there (it shows in the launcher as always).
    pub fn ov_toast(&mut self, toast: &ToastData, seq: u64) -> bool {
        if !self.in_game() || self.ov_helper != Helper::Ready
            || overlay::desktop() == Desktop::SteamGameMode {
            return false;
        }
        crate::log!("overlay: notification over the game: {}", toast.title);
        let w = self.ov_window();
        self.ov_style();
        w.set_toast(toast.clone());
        w.set_toast_shown(true);
        if !self.ov_open {
            w.set_open(false);
            let _ = w.show();
            Self::ov_fullscreen(&w);
            // Clicks go to the game.
            w.window().with_winit_window(|win: &winit::window::Window| {
                let _ = win.set_cursor_hittest(false);
                win.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
            });
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(60));
                overlay::raise_overlay(false);
            });
        }
        slint::Timer::single_shot(Duration::from_secs(5), move || with_app(move |app| {
            if app.toast_seq != seq { return; }
            if let Some(w) = &app.ov {
                w.set_toast_shown(false);
                let w2 = w.as_weak();
                // Gone once it has slid away, unless the panel opened meanwhile.
                slint::Timer::single_shot(Duration::from_millis(400), move || with_app(move |app| {
                    if !app.ov_open && app.toast_seq == seq { if let Some(w) = w2.upgrade() { let _ = w.hide(); } }
                }));
            }
        }));
        true
    }

    /// Settings → Playing: the overlay's rows.
    pub fn overlay_rows(&self, cfg: &crate::config::Config) -> Vec<(SId, SettingData)> {
        let row = |kind: i32, label: &str| SettingData { kind, label: label.into(), ..Default::default() };
        let desk = overlay::desktop();
        let mut r = row(4, "In-game overlay");
        (r.value, r.value_kind) = match self.ov_helper {
            Helper::Ready => ("Ready".into(), 1),
            Helper::Missing => ("Install the GNOME helper".into(), 3),
            Helper::NeedsLogin => ("Log out and in to finish".into(), 3),
            Helper::Steam => ("Steam's overlay is used".into(), 0),
            Helper::Unsupported => (format!("Not on {}", desk.label()).into(), 0),
        };
        r.hint = "PS in a game: the panel over it · hold PS: Home · Share: a screenshot · keyboard: bind swarmplay --overlay to a key".into();
        let mut rows = vec![(SId::OverlayHelper, r)];
        let mut r = row(2, "Pause the game while the overlay is open");
        r.on = cfg.overlay_pause;
        r.hint = "The game stops under the panel (a few games' sound may stutter)".into();
        rows.push((SId::OverlayPause, r));
        rows
    }

    pub fn overlay_helper_pressed(&mut self) {
        match self.ov_helper {
            Helper::Missing => match overlay::install_gnome_helper() {
                Ok(()) => {
                    audio::play(Sound::Select);
                    self.ov_helper = Helper::NeedsLogin;
                    self.note_setting(SId::OverlayHelper, "Installed · log out and in once, then press PS in a game", 1);
                    self.refresh_settings();
                }
                Err(e) => self.note_setting(SId::OverlayHelper, &format!("Couldn't install it: {e}"), 2),
            },
            Helper::NeedsLogin => self.note_setting(SId::OverlayHelper, "GNOME loads it at the next login: log out and in once", 0),
            Helper::Ready => self.note_setting(SId::OverlayHelper, "Ready: press PS while a game runs", 1),
            Helper::Steam => self.note_setting(SId::OverlayHelper, "In Steam's Game Mode, the PS button opens Steam's overlay", 0),
            Helper::Unsupported => self.note_setting(SId::OverlayHelper, &format!("{} has no way to show it over games: PS brings SwarmPlay up instead", overlay::desktop().label()), 0),
        }
    }
}

