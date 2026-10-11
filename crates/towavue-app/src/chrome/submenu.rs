use egui::containers::menu::{MenuConfig, MenuState, SubMenu};
use egui::{Align, InnerResponse, Layout, Popup, PopupCloseBehavior, Response, Ui};

/// Retain completed content bounds, including scroll gutters and row overflow.
pub fn remember_menu_bounds(ui: &Ui) {
    let id = egui::containers::menu::find_menu_root(ui).id;
    let bounds = (ui.ctx().cumulative_pass_nr(), ui.min_rect());
    ui.data_mut(|data| data.insert_temp(id.with("cascade-bounds"), bounds));
}

pub fn chevron() -> egui::RichText {
    egui::RichText::new("\u{eab6}").font(crate::fonts::icon_font())
}

pub fn menu_button<R>(
    ui: &mut Ui,
    text: impl Into<egui::WidgetText>,
    content: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<Option<R>> {
    let text = text.into();
    let (response, inner) =
        submenu_button(ui, egui::Button::new(text).right_text(chevron()), content);
    InnerResponse::new(inner.map(|inner| inner.inner), response)
}

pub fn submenu_button<R>(
    ui: &mut Ui,
    button: egui::Button<'_>,
    content: impl FnOnce(&mut Ui) -> R,
) -> (Response, Option<InnerResponse<R>>) {
    let id = SubMenu::id_from_widget_id(ui.next_auto_id());
    let open = MenuState::from_ui(ui, |state, _| state.open_item == Some(id));
    let inactive = ui.visuals().widgets.inactive;
    if open {
        ui.visuals_mut().widgets.inactive = ui.visuals().widgets.open;
    }
    let response = ui.add(button);
    ui.visuals_mut().widgets.inactive = inactive;
    ui.ctx().accesskit_node_builder(response.id, |node| {
        if let Some(label) = node.label() {
            let label = label.trim_end_matches('\u{eab6}').trim_end().to_owned();
            node.set_label(label);
        }
    });
    let inner = submenu(ui, &response, None, content);
    (response, inner)
}

/// Preserve egui's menu ownership and pointer/keyboard rules while controlling
/// cascade geometry. No separate popup state or input dispatcher is introduced.
pub fn submenu<R>(
    ui: &Ui,
    button: &Response,
    floor: Option<f32>,
    content: impl FnOnce(&mut Ui) -> R,
) -> Option<InnerResponse<R>> {
    let frame = egui::Frame::menu(ui.style());
    let id = SubMenu::id_from_widget_id(button.id);
    let (open_item, parent_id) = MenuState::from_ui(ui, |state, stack| (state.open_item, stack.id));
    let parent = ui.ctx().read_response(parent_id)?;
    let config = MenuConfig::find(ui);
    let hover = ui.ctx().pointer_hover_pos();
    let hovered = hover.is_some_and(|pos| {
        button
            .rect
            .expand2(ui.spacing().item_spacing * 0.5)
            .contains(pos)
    });
    let mut open = open_item == Some(id);
    let mut set_open = None;
    if button.enabled()
        && open
        && button.clicked()
        && !button.clicked_by(egui::PointerButton::Primary)
    {
        open = false;
        set_open = Some(false);
    } else if button.enabled() && ((!open && button.clicked()) || (hovered && open_item.is_none()))
    {
        open = true;
        set_open = Some(true);
        MenuState::from_id(ui.ctx(), parent_id, |state| state.open_item = None);
    }
    let screen = ui.ctx().content_rect();
    let floor = floor
        .unwrap_or(screen.top())
        .max(screen.top())
        .min(screen.bottom());
    // Area's retained response can still include a wider loading/sizing pass.
    // Use the completed content bounds from the preceding pass, which match
    // Frame's painted border rather than Area's retained extent or a scroll
    // area's narrower viewport. Initial sizing uses the current constraint.
    let content_bounds = ui
        .data(|data| data.get_temp::<(u64, egui::Rect)>(parent_id.with("cascade-bounds")))
        .filter(|(pass, _)| ui.ctx().cumulative_pass_nr().saturating_sub(*pass) <= 1)
        .map_or_else(|| ui.max_rect(), |(_, bounds)| bounds);
    // The parent may itself have moved to fit the screen this pass. Retain only
    // its measured width; its current left edge is already known.
    let content_bounds = egui::Rect::from_min_size(ui.max_rect().min, content_bounds.size());
    let parent_frame = content_bounds.expand2(egui::vec2(frame.total_margin().sum().x * 0.5, 0.0));
    let right = (screen.right() - parent_frame.right() + 1.0).max(1.0);
    let left = (parent_frame.left() - screen.left() + 1.0).max(1.0);
    let expected = egui::AreaState::load(ui.ctx(), id)
        .and_then(|state| state.size)
        .unwrap_or_default();
    let use_right = (expected.x > 0.0 && expected.x <= right) || right >= left;
    let room = if use_right { right } else { left };
    let margin = frame.total_margin().sum();
    let max_size = egui::vec2(
        (room - margin.x).max(1.0),
        (screen.bottom() - floor - margin.y).max(1.0),
    );
    let x = if use_right {
        parent_frame.right() - 1.0
    } else {
        parent_frame.left() + 1.0 - expected.x.min(room)
    };
    let y = (button.rect.top() - frame.total_margin().top)
        .min(screen.bottom() - expected.y.min(screen.bottom() - floor))
        .max(floor);
    let popup = Popup::from_response(button)
        .id(id)
        .open(open)
        .at_position(egui::pos2(x, y))
        .align(egui::emath::RectAlign::BOTTOM_START)
        .align_alternatives(&[])
        .gap(0.0)
        .layout(Layout::top_down_justified(Align::Min))
        .style(super::menu_style)
        .frame(frame)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .info(
            egui::UiStackInfo::new(egui::UiKind::Menu)
                .with_tag_value(MenuConfig::MENU_CONFIG_TAG, config.clone()),
        )
        .show(|child| {
            // egui only supports one sublayer level. Flatten the cascade under
            // its root; promoting every descendant preserves their creation
            // order when clicking a parent would otherwise raise it above them.
            let pass = child.ctx().cumulative_pass_nr();
            let root = ui
                .data(|data| {
                    data.get_temp::<(u64, egui::LayerId)>(ui.layer_id().id.with("cascade-root"))
                })
                .filter(|(saved, _)| *saved == pass)
                .map_or(ui.layer_id(), |(_, root)| root);
            child.ctx().set_sublayer(root, child.layer_id());
            child.ctx().move_to_top(child.layer_id());
            child.data_mut(|data| {
                data.insert_temp(child.layer_id().id.with("cascade-root"), (pass, root))
            });
            child.set_max_size(max_size);
            let inner = content(child);
            remember_menu_bounds(child);
            inner
        });
    if let Some(popup) = &popup {
        let deepest = MenuState::is_deepest_open_sub_menu(ui.ctx(), id);
        let outside = deepest && popup.response.clicked_elsewhere() && parent.clicked_elsewhere();
        let inside = deepest
            && !button.clicked()
            && ui.input(|input| input.pointer.any_click())
            && hover.is_some_and(|pos| popup.response.interact_rect.contains(pos));
        let close = match config.close_behavior {
            PopupCloseBehavior::CloseOnClick => outside || inside,
            PopupCloseBehavior::CloseOnClickOutside => outside,
            PopupCloseBehavior::IgnoreClicks => false,
        };
        if close || popup.response.should_close() {
            set_open = Some(false);
            ui.close();
        }
        let moving = ui.input(|input| input.pointer.is_moving_towards_rect(&popup.response.rect));
        if moving {
            ui.ctx().request_repaint();
        }
        let in_parent = hover.is_some_and(|pos| {
            parent.rect.contains(pos) && ui.ctx().layer_id_at(pos) == Some(parent.layer_id)
        });
        if open && !hovered && !popup.response.contains_pointer() && !moving && in_parent {
            set_open = Some(false);
        }
        if ui.will_parent_close() {
            ui.data_mut(|data| data.remove_by_type::<MenuState>());
        }
    }
    if let Some(open) = set_open {
        MenuState::from_id(ui.ctx(), parent_id, |state| {
            state.open_item = open.then_some(id)
        });
    }
    popup
}
