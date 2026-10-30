use super::*;
use shinki_emu_manager_ui_slint::StoreAction;
use shinki_emu_manager_ui_slint::tr;

/// Store sub-page: every UI callback arrives as one [`StoreAction`]. The library
/// folder picker is a host service, so `ChooseLibraryRoot` becomes
/// `SetLibraryRoot` once the user picks a folder.
pub(super) fn install<C: ManagerController>(wiring: &Wiring<C>) {
    let Some(adapter) = wiring.adapter.upgrade() else {
        return;
    };
    let w = wiring.clone();
    adapter.set_on_store_action(move |action| match action {
        StoreAction::ChooseLibraryRoot => {
            let w = w.clone();
            platform::pick_folder(
                tr!("选择 Store 游戏库目录"),
                Box::new(move |picked| {
                    w.run(|c| {
                        let path = picked?.to_string_lossy().into_owned();
                        c.store_action(StoreAction::SetLibraryRoot { path })
                    })
                }),
            );
        }
        action => w.run(|c| c.store_action(action)),
    });
}
