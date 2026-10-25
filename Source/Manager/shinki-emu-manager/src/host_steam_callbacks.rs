use super::*;
use shinki_emu_manager_ui_slint::SteamAction;

/// Steam sub-page: every UI callback arrives as one [`SteamAction`]. The library
/// folder picker is a host service, so `ChooseLibraryRoot` becomes
/// `SetLibraryRoot` once the user picks a folder.
pub(super) fn install<C: ManagerController>(wiring: &Wiring<C>) {
    let Some(adapter) = wiring.adapter.upgrade() else {
        return;
    };
    let w = wiring.clone();
    adapter.set_on_steam_action(move |action| match action {
        SteamAction::ChooseLibraryRoot => {
            let w = w.clone();
            platform::pick_folder(
                "选择 Steam 游戏库目录",
                Box::new(move |picked| {
                    w.run(|c| {
                        let path = picked?.to_string_lossy().into_owned();
                        c.steam_action(SteamAction::SetLibraryRoot { path })
                    })
                }),
            );
        }
        action => w.run(|c| c.steam_action(action)),
    });
}
