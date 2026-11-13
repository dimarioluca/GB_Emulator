slint::include_modules!();

use std::process::Command;

fn show_error(weak_ui: slint::Weak<LauncherWindow>, title: &str, description: String) {
    let Some(ui) = weak_ui.upgrade() else {
        return;
    };
    ui.set_busy(true);

    let dialog = rfd::AsyncMessageDialog::new()
        .set_title(title)
        .set_description(description)
        .set_parent(&ui.window().window_handle());

    slint::spawn_local(async move {
        dialog.show().await;
        if let Some(ui) = weak_ui.upgrade() {
            ui.set_busy(false);
        }
    })
    .ok();
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = LauncherWindow::new()?;

    let open_rom_ui = ui.as_weak();
    // flag busy to avoid disable clicking buttons while file dialog is open
    let on_open_rom = move || {
        let Some(ui) = open_rom_ui.upgrade() else {
            return;
        };
        if ui.get_busy() {
            return;  // there is already a file dialog open
        }
        ui.set_busy(true);

        let dialog = rfd::AsyncFileDialog::new()
            .add_filter("Game Boy ROM", &["gb", "gbc"])
            .set_parent(&ui.window().window_handle());

        let async_ui = open_rom_ui.clone();
        slint::spawn_local(async move {
            let file = dialog.pick_file().await;
            if let Some(ui) = async_ui.upgrade() {
                ui.set_busy(false);
                if let Some(handle) = file {
                    ui.set_rom_path(handle.path().to_string_lossy().to_string().into());
                }
            }
        })
        .ok();
    };
    ui.on_open_rom_clicked(on_open_rom);

    let start_emu_ui = ui.as_weak();
    let on_start_emulator = move || {
        let Some(ui) = start_emu_ui.upgrade() else {
            return;
        };
        if ui.get_busy() {
            return;
        }
        let path = ui.get_rom_path().to_string();
        if path.is_empty() {
            return;
        }

        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                show_error(
                    start_emu_ui.clone(),
                    "Error",
                    format!("Executable not found: {e}"),
                );
                return;
            }
        };

        match Command::new(exe).arg("--emulator").arg(&path).spawn() {
            Ok(_) => {
                ui.hide().ok();
            }
            Err(e) => {
                show_error(
                    start_emu_ui.clone(),
                    "Error starting emulator",
                    e.to_string(),
                );
            }
        }
    };
    ui.on_start_emulator_clicked(on_start_emulator);

    ui.run()
}