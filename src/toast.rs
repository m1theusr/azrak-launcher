//! Notifications, as a console's: a small card slides in at the top right ("Ready to play",
//! "Downloaded"), stays five seconds and goes. A newer one takes its place.

use crate::app::*;
use crate::ToastData;
use std::time::Duration;

/// How a notification is coloured (ui/app.slint, which also has 2: needs a look): what it's
/// about, done, failed.
pub const INFO: i32 = 0;
pub const DONE: i32 = 1;
pub const FAILED: i32 = 3;

impl App {
    /// Show `title` with `sub` under it; `icon` is an Icons.get name.
    pub fn toast(&mut self, title: &str, sub: &str, icon: &str, kind: i32) {
        self.toast_seq += 1;
        let seq = self.toast_seq;
        let ui = self.ui();
        let data = ToastData { title: title.into(), sub: sub.into(), icon: icon.into(), kind };
        ui.set_toast(data.clone());
        ui.set_toast_shown(true);
        // With a game in front, over the game too.
        self.ov_toast(&data, seq);
        slint::Timer::single_shot(Duration::from_secs(5), move || with_app(move |app| {
            // Still this one (not one shown since): it goes.
            if app.toast_seq == seq { app.ui().set_toast_shown(false); }
        }));
    }
}
