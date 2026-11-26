use super::*;
use shinki_emu_manager_ui_slint::tr;

#[cfg(not(target_env = "ohos"))]
pub(super) fn finish_activation<C: ManagerController>(
    controller: &std::rc::Rc<std::cell::RefCell<C>>,
    adapter: &std::rc::Weak<SlintManagerAdapter>,
    event: FilterEvent,
) {
    let (accepted, error) = match event {
        FilterEvent::Activated => (Some(true), None),
        FilterEvent::Rejected(error) => (Some(false), Some(error)),
        FilterEvent::RenderFailed(error) => (None, Some(error)),
    };
    let rollback_error = accepted.and_then(|accepted| {
        match controller.borrow_mut().finish_filter_activation(accepted) {
            Ok(model) => {
                if let Some(model) = model {
                    apply_model(adapter, &model);
                }
                None
            }
            Err(error) => Some(error),
        }
    });
    if let Some(adapter) = adapter.upgrade() {
        if let Some(error) = error {
            let error = match rollback_error {
                Some(rollback) => format!("{error}; {rollback}"),
                None => error,
            };
            report_error(adapter.window(), &error);
        }
        if accepted == Some(false) {
            // The rejected encoder was discarded; give the previous chain a
            // chance to present even if the core publishes no further frame.
            adapter.window().window().request_redraw();
        }
    }
}

type FilterChange<'a, C> = dyn Fn(&mut C, FilterApply<'_>) -> Result<ManagerViewModel, String> + 'a;

/// Run a filter change through the renderer; a failed parameter shows under its field.
fn wire<C: ManagerController, R: ShinkiUnderlayRenderer>(
    wiring: &Wiring<C>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
) -> impl Fn(Option<&str>, &FilterChange<'_, C>) + Clone + use<C, R> {
    let wiring = wiring.clone();
    let renderer = renderer.clone();
    move |field, change| {
        let action = |c: &mut C| {
            change(c, &mut |settings| {
                renderer.borrow_mut().configure_filter(settings)
            })
        };
        match field {
            Some(key) => wiring.run_field(FieldScope::Filter, key, action),
            None => wiring.run(action),
        }
        if let Some(window) = wiring.window.upgrade() {
            window.window().request_redraw();
        }
    }
}

pub(super) fn install<C: ManagerController, R: ShinkiUnderlayRenderer>(
    wiring: &Wiring<C>,
    renderer: &std::rc::Rc<std::cell::RefCell<R>>,
) {
    let Some(window) = wiring.window.upgrade() else {
        return;
    };
    let filters = window.global::<Filters>();
    let run = wire(wiring, renderer);
    let set_preset = run.clone();
    filters.on_set_preset(move |preset| {
        set_preset(None, &|c, apply| {
            c.set_filter_preset(preset.as_str(), apply)
        });
    });
    let select = run.clone();
    filters.on_select_stage(move |index| {
        select(None, &|c, apply| {
            c.edit_filter_chain(FilterChainEdit::Select(index), apply)
        });
    });
    let add = run.clone();
    filters.on_add_stage(move |id| {
        add(None, &|c, apply| {
            c.edit_filter_chain(FilterChainEdit::Add(id.as_str()), apply)
        });
    });
    let remove = run.clone();
    filters.on_remove_stage(move |index| {
        remove(None, &|c, apply| {
            c.edit_filter_chain(FilterChainEdit::Remove(index), apply)
        });
    });
    let reorder = run.clone();
    filters.on_move_stage(move |from, to| {
        reorder(None, &|c, apply| {
            c.edit_filter_chain(FilterChainEdit::Move { from, to }, apply)
        });
    });
    let set_parameter = run.clone();
    filters.on_field_changed(move |key, value| {
        set_parameter(Some(key.as_str()), &|c, apply| {
            c.set_filter_parameter(key.as_str(), value.as_str(), apply)
        });
    });
    let reset = run.clone();
    filters.on_reset_parameters(move || reset(None, &|c, apply| c.reset_filter_parameters(apply)));
    filters.on_load_source(move || {
        let run = run.clone();
        platform::pick_file(
            tr!("选择外部画面效果（HLSL）"),
            &[platform::FileFilter {
                label: "HLSL",
                extensions: &["hlsl"],
            }],
            Box::new(move |picked| {
                run(None, &|c, apply| {
                    c.load_filter_source(picked.as_deref().map_err(Clone::clone)?, apply)
                });
            }),
        );
    });
}
