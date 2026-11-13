use slint::{ModelRc, SharedString, StandardListViewItem, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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

fn scan_roms(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut roms = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("gb") || ext.eq_ignore_ascii_case("gbc") {
                    roms.push(path);
                }
            }
        }
    }
    roms.sort();
    Ok(roms)
}

pub fn run() -> Result<(), slint::PlatformError> {
    let ui = LauncherWindow::new()?;

    let rom_paths: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
    let games_model: Rc<VecModel<StandardListViewItem>> = Rc::new(VecModel::default());
    ui.set_games(ModelRc::from(games_model.clone()));

    let on_select_folder_ui = ui.as_weak();
    let folder_rom_paths = rom_paths.clone();
    let folder_games_model = games_model.clone();
    // flag busy to avoid disable clicking buttons while file dialog is open
    let on_select_folder = move || {
        let Some(ui) = on_select_folder_ui.upgrade() else {
            return;
        };
        if ui.get_busy() {
            return;  // there is already a file dialog open
        }
        ui.set_busy(true);

        let dialog = rfd::AsyncFileDialog::new()
            .set_parent(&ui.window().window_handle());

        let async_paths = folder_rom_paths.clone();
        let async_model = folder_games_model.clone();

        let async_ui = on_select_folder_ui.clone();
        slint::spawn_local(async move {
            let file = dialog.pick_folder().await;
            if let Some(ui) = async_ui.upgrade() {
                ui.set_busy(false);
                if let Some(handle) = file {
                    let dir_path = handle.path().to_path_buf();
                    ui.set_folder_path(dir_path.to_string_lossy().to_string().into());

                    match scan_roms(&dir_path) {
                        Ok(found_roms) => {
                            let items: Vec<StandardListViewItem> = found_roms
                                .iter()
                                .filter_map(|p| p.file_name())
                                .map(|name| {
                                    StandardListViewItem::from(SharedString::from(
                                        name.to_string_lossy().as_ref(),
                                    ))
                                })
                                .collect();

                            async_model.set_vec(items);
                            let is_empty = found_roms.is_empty();
                            *async_paths.borrow_mut() = found_roms;

                            ui.set_selected_index(if is_empty { -1 } else { 0 });
                        }
                        Err(e) => {
                            show_error(
                                async_ui.clone(),
                                "Error reading folder",
                                e.to_string(),
                            );
                        }
                    }
                    
                }
            }
        })
        .ok();
    };
    ui.on_select_folder_clicked(on_select_folder);

    let start_emu_ui = ui.as_weak();
    let start_rom_paths = rom_paths.clone();
    let on_start_emulator = move || {
        let Some(ui) = start_emu_ui.upgrade() else {
            return;
        };
        if ui.get_busy() {
            return;
        }
        let idx = ui.get_selected_index();
        if idx < 0 {
            return;
        }
        let Some(path) = start_rom_paths.borrow().get(idx as usize).cloned() else {
            return;
        };

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