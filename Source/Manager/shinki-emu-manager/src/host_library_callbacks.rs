use super::*;
use shinki_emu_manager_ui_slint::tr;

pub(super) fn install<C: ManagerController>(wiring: &Wiring<C>) {
    let Some(window) = wiring.window.upgrade() else {
        return;
    };
    install_library(wiring, &window);
    install_selection(wiring, &window);
    install_game_settings(wiring, &window);
    install_patch(wiring, &window);
    let w = wiring.clone();
    window.global::<Logs>().on_load(move |core| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        match w.controller.borrow().log_text(core.as_str()) {
            Ok((text, count)) => {
                let logs = window.global::<Logs>();
                logs.set_text(if count == 0 {
                    tr!("暂无相关日志").into()
                } else {
                    text.into()
                });
                logs.set_count(i32::try_from(count).unwrap_or(i32::MAX));
            }
            Err(error) => report_error(&window, &error),
        }
    });
}

fn install_library<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
) {
    let library = window.global::<Library>();
    let w = wiring.clone();
    library.on_prepare(move || w.run(|c| c.refresh_library()));
    let w = wiring.clone();
    library.on_cancel_prepare(move || w.run(|c| c.cancel_library_scan()));
    let w = wiring.clone();
    library.on_add_directory(move || {
        let w = w.clone();
        platform::pick_library_folder(
            tr!("选择游戏目录"),
            Box::new(move |picked| w.run(|c| c.add_game_directory(&picked?))),
        );
    });
    library.set_optical_available(platform::OPTICAL_VOLUMES_SUPPORTED);
    #[cfg(any(
        windows,
        target_os = "macos",
        all(target_os = "linux", not(target_env = "ohos"))
    ))]
    optical::install(wiring, &library);
    let w = wiring.clone();
    library.on_search(move |query| w.run(|c| c.search(query.as_str())));
    let w = wiring.clone();
    library.on_set_sort(move |mode| w.run(|c| c.set_library_sort(mode.as_str())));
    let w = wiring.clone();
    library
        .on_set_compat_filter(move |filter| w.run(|c| c.set_compatibility_filter(filter.as_str())));
    let w = wiring.clone();
    library.on_select(move |game_id| w.run(|c| c.select_case(game_id.as_str())));
    let w = wiring.clone();
    library.on_open_game_settings(move |game_id| w.run(|c| c.select_case(game_id.as_str())));
    let w = wiring.clone();
    library.on_open_folder(move |game_id| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let result = w
            .controller
            .borrow()
            .game_location(game_id.as_str())
            .and_then(|location| platform::open_path(&location));
        if let Err(error) = result {
            report_error(&window, &error);
        }
    });
    let w = wiring.clone();
    library.on_remove(move |game_id| w.run(|c| c.remove_from_library(game_id.as_str())));
}

/// 打开光驱: list mounted data discs off the UI thread, import the only one,
/// or let the user pick one when several are mounted.
#[cfg(any(
    windows,
    target_os = "macos",
    all(target_os = "linux", not(target_env = "ohos"))
))]
mod optical {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    type Listed = Box<dyn FnOnce(Result<Vec<platform::OpticalVolume>, String>)>;

    /// The discs behind `Library.optical-choices`. `import-optical(index)`
    /// only indexes this snapshot; closing the dialog or a new listing
    /// replaces it, so a stale row never imports another disc's root.
    type Snapshot = Rc<RefCell<Vec<platform::OpticalVolume>>>;

    thread_local! {
        // Slint event-loop thread only: a listing starts there and its worker
        // delivers the result there through `slint::invoke_from_event_loop`.
        static LISTED: RefCell<Option<Listed>> = const { RefCell::new(None) };
    }

    pub(super) fn install<C: ManagerController>(wiring: &Wiring<C>, library: &Library<'_>) {
        let snapshot = Snapshot::default();
        let w = wiring.clone();
        let listed = snapshot.clone();
        library.on_open_optical(move || {
            let Some(window) = w.window.upgrade() else {
                return;
            };
            let library = window.global::<Library>();
            if library.get_optical_listing() {
                return;
            }
            show_choices(&library, &listed, Vec::new());
            let done_wiring = w.clone();
            let done_snapshot = listed.clone();
            LISTED.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |result| {
                    finish(&done_wiring, &done_snapshot, result);
                }));
            });
            library.set_optical_listing(true);
            // Reading a volume label can wait for the drive to spin up.
            let spawned = std::thread::Builder::new()
                .name("manager-optical-volumes".into())
                .spawn(|| {
                    let result = platform::optical_volumes();
                    if let Err(error) = slint::invoke_from_event_loop(move || {
                        if let Some(done) = LISTED.with(|slot| slot.borrow_mut().take()) {
                            done(result);
                        }
                    }) {
                        tracing::warn!(
                            event = "shinki.emu.manager.optical_listing_dropped",
                            error = %error
                        );
                    }
                });
            if let Err(error) = spawned {
                LISTED.with(|slot| slot.borrow_mut().take());
                library.set_optical_listing(false);
                report_error(&window, &format!("SHINKI_EMU_OPTICAL_VOLUMES: {error}"));
            }
        });
        let w = wiring.clone();
        let picked = snapshot.clone();
        library.on_import_optical(move |index| {
            let Some(window) = w.window.upgrade() else {
                return;
            };
            let mut volumes = picked.take();
            show_choices(&window.global::<Library>(), &picked, Vec::new());
            let Some(index) = usize::try_from(index)
                .ok()
                .filter(|index| *index < volumes.len())
            else {
                return;
            };
            let volume = volumes.swap_remove(index);
            w.run(|c| c.add_optical_game_directory(volume));
        });
        let w = wiring.clone();
        library.on_close_optical(move || {
            if let Some(window) = w.window.upgrade() {
                show_choices(&window.global::<Library>(), &snapshot, Vec::new());
            }
        });
    }

    fn finish<C: ManagerController>(
        wiring: &Wiring<C>,
        snapshot: &Snapshot,
        result: Result<Vec<platform::OpticalVolume>, String>,
    ) {
        let Some(window) = wiring.window.upgrade() else {
            return;
        };
        let library = window.global::<Library>();
        library.set_optical_listing(false);
        match result {
            Err(error) => report_error(&window, &error),
            Ok(volumes) if volumes.is_empty() => {
                report_error(&window, "SHINKI_EMU_OPTICAL_NO_MEDIA");
            }
            Ok(mut volumes) if volumes.len() == 1 => {
                let volume = volumes.swap_remove(0);
                wiring.run(|c| c.add_optical_game_directory(volume));
            }
            Ok(volumes) => show_choices(&library, snapshot, volumes),
        }
    }

    fn show_choices(
        library: &Library<'_>,
        snapshot: &Snapshot,
        volumes: Vec<platform::OpticalVolume>,
    ) {
        let choices: Vec<shinki_emu_manager_ui_slint::Choice> = volumes
            .iter()
            .map(|volume| shinki_emu_manager_ui_slint::Choice {
                id: volume.root.display().to_string().into(),
                label: volume.label.as_deref().unwrap_or_default().into(),
            })
            .collect();
        library.set_optical_choices(slint::ModelRc::new(slint::VecModel::from(choices)));
        *snapshot.borrow_mut() = volumes;
    }
}

fn install_selection<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
) {
    let selection = window.global::<Selection>();
    let w = wiring.clone();
    selection.on_choose_emulator(move |plugin_id| {
        let Some(window) = w.window.upgrade() else {
            return;
        };
        let game_id = window.global::<Selection>().get_game().id;
        w.run(|c| c.set_game_emulator(game_id.as_str(), plugin_id.as_str()));
    });
    let w = wiring.clone();
    selection.on_pin_release(move |release_id| w.run(|c| c.pin_release(release_id.as_str())));
    let w = wiring.clone();
    selection.on_fetch_releases(move || w.run(|c| c.fetch_releases()));
    let w = wiring.clone();
    selection.on_sync_bangumi(move |progress| {
        w.run(|c| {
            c.update_bangumi_play_status(
                progress.status.as_str(),
                progress.rating,
                progress.note.as_str(),
            )
        })
    });
    let w = wiring.clone();
    selection.on_search_metadata(move |provider, query| {
        w.run(|c| c.search_metadata(provider.as_str(), query.as_str()))
    });
    let w = wiring.clone();
    selection.on_accept_match(move |candidate| w.run(|c| c.accept_match(candidate.as_str())));
    let w = wiring.clone();
    selection.on_refresh_metadata(move |provider| w.run(|c| c.refresh_metadata(provider.as_str())));
    let w = wiring.clone();
    selection.on_unlink_metadata(move |provider| w.run(|c| c.unlink_identity(provider.as_str())));
}

fn game_id(
    window: &slint::Weak<shinki_emu_manager_ui_slint::ManagerWindow>,
) -> Option<slint::SharedString> {
    window
        .upgrade()
        .map(|window| window.global::<GameSettings>().get_game_id())
}

fn install_game_settings<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
) {
    let settings = window.global::<GameSettings>();
    let w = wiring.clone();
    settings.on_close(move || w.run(|c| c.cancel_key_capture()));
    let w = wiring.clone();
    settings.on_prepare_configuration(move || {
        if let Some(game) = game_id(&w.window) {
            w.run(|c| c.prepare_game_configuration(game.as_str()));
        }
    });
    let w = wiring.clone();
    settings.on_set_emulator(move |plugin_id| {
        if let Some(game) = game_id(&w.window) {
            w.run(|c| c.set_game_emulator(game.as_str(), plugin_id.as_str()));
        }
    });
    let w = wiring.clone();
    settings.on_set_render(move |mode, width, height| {
        let Some(game) = game_id(&w.window) else {
            return;
        };
        w.run_field(FieldScope::GameRender, "", |c| {
            let preference = render_choice(mode.as_str(), width, height)?;
            c.set_render_preference(game.as_str(), preference)
        });
    });
    let w = wiring.clone();
    settings.on_field_changed(move |key, value| {
        if let Some(game) = game_id(&w.window) {
            w.run_field(FieldScope::Game, key.as_str(), |c| {
                c.set_game_config(game.as_str(), key.as_str(), value.as_str())
            });
        }
    });
    let w = wiring.clone();
    settings.on_reset_field(move |key| {
        if let Some(game) = game_id(&w.window) {
            w.run_field(FieldScope::Game, key.as_str(), |c| {
                c.reset_game_config(game.as_str(), key.as_str())
            });
        }
    });
    let w = wiring.clone();
    settings.on_pick_path(move |key| {
        let Some(game) = game_id(&w.window) else {
            return;
        };
        let request = w
            .controller
            .borrow()
            .config_path_pick(game.as_str(), key.as_str());
        let request = match request {
            Ok(request) => request,
            Err(error) => return w.run_field(FieldScope::Game, key.as_str(), |_| Err(error)),
        };
        let w = w.clone();
        let done: platform::PickDone = Box::new(move |picked| {
            w.run_field(FieldScope::Game, key.as_str(), |c| {
                c.set_config_path(game.as_str(), key.as_str(), &picked?)
            });
        });
        if request.directory {
            platform::pick_folder(&request.title, done);
        } else if let Some(filter) = request.filter {
            let extensions: Vec<_> = filter.extensions.iter().map(String::as_str).collect();
            platform::pick_file(
                &request.title,
                &[platform::FileFilter {
                    label: &filter.label,
                    extensions: &extensions,
                }],
                done,
            );
        } else {
            platform::pick_file(&request.title, &[], done);
        }
    });
    let w = wiring.clone();
    settings.on_apply_recommended(move || {
        if let Some(game) = game_id(&w.window) {
            w.run(|c| c.apply_recommended_config(game.as_str()));
        }
    });
    let w = wiring.clone();
    settings.on_set_input_override(move |enabled| {
        if let Some(game) = game_id(&w.window) {
            w.run_mapping(|c| c.set_game_input_override(game.as_str(), enabled));
        }
    });
}

fn install_patch<C: ManagerController>(
    wiring: &Wiring<C>,
    window: &shinki_emu_manager_ui_slint::ManagerWindow,
) {
    let patch = window.global::<Patch>();
    let w = wiring.clone();
    patch.on_search(move |query| w.run(|c| c.search_patch_games(query.as_str())));
    let w = wiring.clone();
    patch.on_select_game(move |game_id| w.run(|c| c.select_patch_game(game_id.as_str())));
    let w = wiring.clone();
    patch.on_pick_script(move || {
        let w = w.clone();
        platform::pick_file(
            tr!("选择本地补丁（.luau 或 .zip）"),
            &[platform::FileFilter {
                label: tr!("Luau 补丁"),
                extensions: &["luau", "zip"],
            }],
            Box::new(move |picked| w.run(|c| c.set_patch_script(&picked?))),
        );
    });
    let w = wiring.clone();
    patch.on_select_recipe(move |id| w.run(|c| c.select_patch_recipe(id.as_str())));
    let w = wiring.clone();
    patch.on_set_mode(move |in_place| w.run(|c| c.set_patch_mode(in_place)));
    let w = wiring.clone();
    patch.on_restore(move || w.run(|c| c.restore_patch()));
    let w = wiring.clone();
    patch.on_pick_directory(move |kind| {
        let w = w.clone();
        let output = kind == "output";
        let done: platform::PickDone =
            Box::new(move |picked| w.run(|c| c.set_patch_directory(kind.as_str(), &picked?)));
        if output {
            // The native folder UI selects the parent. The proposed child is
            // shown in output_directory and created by the patch transaction.
            platform::pick_new_directory(
                tr!("选择输出位置（将在其中新建转换目录）"),
                "localized-output",
                done,
            );
        } else {
            platform::pick_folder(tr!("选择补丁输入目录"), done);
        }
    });
    let w = wiring.clone();
    patch.on_start(move || w.run(|c| c.start_patch()));
    let w = wiring.clone();
    patch.on_cancel(move || w.run(|c| c.cancel_file_patch()));
    let w = wiring.clone();
    patch.on_open_output(move || {
        let result = w.controller.borrow_mut().open_patch_output();
        let opened = result.is_ok();
        w.show(result);
        if let (true, Some(window)) = (opened, w.window.upgrade()) {
            window.global::<App>().set_page("library".into());
            window.global::<Library>().set_detail_open(true);
        }
    });
}

/// `RenderPicker` choice plus custom size; negative sizes are invalid.
pub(super) fn render_choice(
    mode: &str,
    width: i32,
    height: i32,
) -> Result<Option<shinki_emu_manager_core::RenderPreference>, String> {
    let size = |value: i32| {
        u32::try_from(value).map_err(|_| "SHINKI_EMU_RENDER_PREFERENCE_INVALID".to_owned())
    };
    shinki_emu_manager_core::RenderPreference::from_choice(mode, size(width)?, size(height)?)
}
