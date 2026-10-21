#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Install a panic hook that shows a native OS dialog so the user sees
    // what went wrong instead of the app silently vanishing.
    abigail_app::install_panic_dialog_hook();

    // run() internally calls try_run() and shows a dialog on error.
    abigail_app::run();
}
