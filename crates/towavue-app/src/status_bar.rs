use crate::*;

#[cfg(test)]
mod tests;

/// Keep transient actions and their message inside the same inset as status text.
pub(super) fn notice_row<R>(
    ui: &mut egui::Ui,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let inset = chrome::TAB_PADDING.min(ui.max_rect().width().max(0.0) * 0.5);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(ui.max_rect().shrink2(egui::vec2(inset, 0.0))),
        |ui| {
            chrome::flat_buttons(ui);
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.spacing_mut().button_padding = egui::Vec2::ZERO;
            ui.spacing_mut().interact_size = egui::Vec2::splat(chrome::STATUS_BUTTON_SIZE);
            ui.horizontal_centered(content).inner
        },
    )
}

pub(super) fn width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.into(),
            egui::FontId::proportional(chrome::TEXT_LG),
            chrome::MUTED,
        )
        .size()
        .x
        + chrome::TAB_PADDING * 2.0
}

pub(super) fn item(
    ui: &mut egui::Ui,
    label: impl Into<egui::WidgetText>,
    clickable: bool,
) -> egui::Response {
    let label = label.into();
    let galley = label.clone().into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::FontId::proportional(chrome::TEXT_LG),
    );
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(
            (galley.size().x + chrome::TAB_PADDING * 2.0).min(ui.available_width()),
            chrome::ROW_HEIGHT,
        ),
        egui::Sense::hover(),
    );
    item_at(ui, rect, label, clickable)
}

pub(super) fn item_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    label: impl Into<egui::WidgetText>,
    clickable: bool,
) -> egui::Response {
    item_at_tinted(
        ui,
        rect,
        label,
        clickable,
        chrome::MUTED,
        chrome::FOREGROUND,
    )
}

/// Apply state colors only to placeholder text; explicit count/accent colors stay fixed.
pub(super) fn item_at_tinted(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    label: impl Into<egui::WidgetText>,
    clickable: bool,
    idle_color: egui::Color32,
    hot_color: egui::Color32,
) -> egui::Response {
    let label = label.into();
    let text = label.text().to_owned();
    let galley = label.into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (rect.width() - 2.0 * chrome::TAB_PADDING).max(0.0),
        egui::FontId::proportional(chrome::TEXT_LG),
    );
    let mut rect = rect;
    if !text.is_empty() {
        let width = (galley.size().x + 2.0 * chrome::TAB_PADDING).min(rect.width());
        if ui.layout().main_dir() == egui::Direction::RightToLeft {
            rect.min.x = rect.right() - width;
        } else {
            rect.max.x = rect.left() + width;
        }
    }
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let response = ui.interact(
        rect,
        id,
        if clickable {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            if clickable {
                egui::WidgetType::Button
            } else {
                egui::WidgetType::Label
            },
            response.enabled(),
            &text,
        )
    });
    let hot = clickable
        && response.enabled()
        && (response.hovered() || response.has_focus() || response.is_pointer_button_down_on());
    let painter = ui.painter().with_clip_rect(
        ui.clip_rect()
            .intersect(rect.expand(1.0 / ui.ctx().pixels_per_point())),
    );
    if hot {
        painter.rect_filled(rect, chrome::RADIUS_XS, chrome::ACTIVE_TAB);
    }
    let color = if hot { hot_color } else { idle_color };
    painter.galley(
        egui::pos2(
            if ui.layout().main_dir() == egui::Direction::RightToLeft {
                ((rect.right() * ui.ctx().pixels_per_point()).round()
                    - ((chrome::TAB_PADDING + galley.size().x) * ui.ctx().pixels_per_point())
                        .round())
                    / ui.ctx().pixels_per_point()
            } else {
                rect.left() + chrome::TAB_PADDING
            },
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    if clickable {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(super) fn icon_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    kind: fluent::Kind,
    selected: bool,
    label: &str,
) -> egui::Response {
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if !response.enabled() {
        chrome::DISABLED
    } else if selected || response.hovered() || response.has_focus() {
        chrome::FOREGROUND
    } else {
        chrome::MUTED
    };
    if kind == fluent::Kind::Dismiss {
        chrome::paint_close(ui, rect, color);
    } else {
        fluent::paint(ui, rect, kind, color);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), label)
    });
    response.help_text(label)
}

pub(super) fn path_menu(
    ui: &egui::Ui,
    response: &egui::Response,
    path: &Path,
    owner: Option<thumbnail_menu::Owner>,
    actions: &mut Vec<UiAction>,
) {
    thumbnail_menu::show(
        ui,
        response,
        path,
        thumbnail_menu::Scope::Status,
        owner,
        actions,
    );
}

impl<N: Fn(AppEvent) + Send + Sync + 'static> Application<N> {
    pub(super) fn status_separator_visible(&self) -> bool {
        !self.timeline_is_visible() && !self.animation_timeline_visible()
    }

    pub(super) fn draw_status_bar(
        &mut self,
        root: &mut egui::Ui,
        actions: &mut Vec<UiAction>,
        volume_targets: &mut Vec<egui::Response>,
    ) -> egui::Rect {
        let language = self.language();
        let mut frame = chrome::bar();
        frame.inner_margin = egui::Margin::same(3);
        let density = root.ctx().pixels_per_point();
        // Anchor to the physical edge, not the sum of independently rounded child widths.
        let info_right = ((root.max_rect().right() * density).round()
            - (f32::from(frame.inner_margin.right) * density).round())
            / density;
        egui::Panel::bottom("status")
            .exact_size(chrome::STATUS_HEIGHT)
            .show_separator_line(self.status_separator_visible())
            .frame(frame)
            .show(root, |ui| {
                // Give padding its own non-focusable target before the controls,
                // so egui's nearest-widget hit assistance cannot click a nearby button.
                ui.interact(
                    ui.max_rect() + frame.inner_margin,
                    ui.id().with("status-padding"),
                    egui::Sense::CLICK | egui::Sense::DRAG,
                );
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = chrome::STATUS_BUTTON_GAP;
                    ui.spacing_mut().interact_size = egui::Vec2::splat(chrome::STATUS_BUTTON_SIZE);
                    ui.spacing_mut().button_padding = egui::Vec2::ZERO;
                    chrome::surface_hover(ui);
                    ui.style_mut().override_font_id =
                        Some(egui::FontId::proportional(chrome::TEXT_LG));
                    if self.filmstrip_open {
                        return;
                    }
                    if self.media_kind.is_none()
                        && !self.keyboard_settings_active()
                        && self.export_notice.is_none()
                        && self.active_export.is_none()
                        && !self.update_is_held()
                    {
                        self.draw_gallery_status(ui);
                        return;
                    }
                    let info = self.status_info();
                    if self.update_is_held() {
                        self.draw_update_status(ui, actions);
                        return;
                    }
                    if self.media_kind.is_some_and(|kind| kind != MediaKind::Image) {
                        let playing = self.state == PlaybackState::Playing;
                        let play = ui
                            .add_enabled_ui(!self.command_context().playback_blocked, |ui| {
                                chrome::transport_button(
                                    ui,
                                    self.track_drag
                                        .as_ref()
                                        .and_then(track_drag::Drag::icon)
                                        .unwrap_or(if playing {
                                            chrome::Icon::Pause
                                        } else {
                                            chrome::Icon::Play
                                        }),
                                    &self.command_hint(
                                        CommandId::TogglePause,
                                        if playing {
                                            localization::Text::Pause.in_language(language)
                                        } else {
                                            localization::Text::PlayReplay.in_language(language)
                                        },
                                    ),
                                )
                            })
                            .inner
                            .disabled_help_text(
                                if self.playback_duration() == Some(Duration::ZERO) {
                                    localization::Text::TimelineEmptyHelp.in_language(language)
                                } else {
                                    localization::Text::PlaybackUnavailableHelp
                                        .in_language(language)
                                },
                            );
                        let dragging = self.track_drag.is_some();
                        if !dragging
                            && self.held_speed.is_none()
                            && let Some((origin, delta)) = track_drag::completed(&play)
                        {
                            actions.push(UiAction::BeginTrackDrag(
                                self.media_generation,
                                origin,
                                delta,
                                true,
                            ));
                        } else if !dragging
                            && self.held_speed.is_none()
                            && play.drag_started_by(egui::PointerButton::Primary)
                            && ui.input(|input| {
                                input.pointer.primary_down() && input.modifiers.is_none()
                            })
                            && let Some((origin, position)) = ui.input(|input| {
                                input
                                    .pointer
                                    .press_origin()
                                    .zip(input.pointer.interact_pos())
                            })
                            && (position.x - origin.x).abs() > (position.y - origin.y).abs()
                        {
                            actions.push(UiAction::BeginTrackDrag(
                                self.media_generation,
                                origin,
                                position - origin,
                                false,
                            ));
                        }
                        let held = !dragging && self.hold_response(&play, actions);
                        if play.clicked() && !held && !dragging {
                            actions.push(UiAction::Command(CommandId::TogglePause));
                        }
                        if self.media_kind == Some(MediaKind::Audio) {
                            self.draw_audio_mode_buttons(ui, actions);
                        } else if chrome::audio_button(
                            ui,
                            chrome::AudioIcon::Repeat,
                            self.video_repeat,
                            &self.command_hint(
                                CommandId::ToggleVideoRepeat,
                                if self.video_repeat {
                                    localization::Text::VideoRepeatOff.in_language(language)
                                } else {
                                    localization::Text::VideoRepeatOn.in_language(language)
                                },
                            ),
                        )
                        .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleVideoRepeat));
                        }
                        let clock_format = if self.timeline_is_visible() {
                            format_time_precise
                        } else {
                            format_time
                        };
                        let duration = self
                            .playback_duration()
                            .map(|duration| clock_format(MediaTime::ZERO.saturating_add(duration)))
                            .unwrap_or_else(|| "—".into());
                        let position = clock_format(self.current_position());
                        let compact = ui.max_rect().width() < 340.0;
                        let mut time_text = egui::text::LayoutJob::default();
                        let format = egui::TextFormat {
                            font_id: egui::FontId::proportional(crate::chrome::TEXT_LG),
                            color: Color32::PLACEHOLDER,
                            ..Default::default()
                        };
                        time_text.append(&position, 0.0, format.clone());
                        if let Some(delta) = self.relative_seek_text() {
                            time_text.append(
                                &format!(" {delta}"),
                                0.0,
                                egui::TextFormat {
                                    color: chrome::FOREGROUND,
                                    ..format.clone()
                                },
                            );
                        }
                        let full_time = format!("{} / {duration}", time_text.text);
                        if !compact {
                            time_text.append(&format!(" / {duration}"), 0.0, format);
                        }
                        let time_width = if compact {
                            (ui.available_width()
                                - self.audio_track_controls_width(ui)
                                - self.video_track_controls_width(ui)
                                - 54.0
                                - if self.active_export.is_some() {
                                    100.0
                                } else {
                                    0.0
                                })
                            .max(0.0)
                        } else {
                            width(ui, &time_text.text)
                        };
                        let (time_rect, _) = ui.allocate_exact_size(
                            egui::vec2(time_width, 24.0),
                            egui::Sense::hover(),
                        );
                        if item_at(ui, time_rect, time_text, true)
                            .help_text(full_time)
                            .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleTimeline));
                        }
                        let volume_label = if self.playback_volume() == 0.0 {
                            chrome::Icon::Muted.text()
                        } else {
                            RichText::new(format!("{:.0}%", self.playback_volume() * 100.0))
                                .size(chrome::TEXT_LG)
                        };
                        let volume = item(ui, volume_label, true).help_text(
                            localization::Text::PlaybackVolumeHelp.in_language(language),
                        );
                        if volume.clicked() {
                            actions.push(UiAction::Command(CommandId::ToggleMute));
                        }
                        volume_targets.push(volume);
                        self.draw_audio_track_controls(ui, actions);
                        self.draw_video_track_controls(ui, actions);
                    } else if self.animation_timeline_visible() {
                        self.draw_animation_transport(ui, actions);
                    } else if self.media_kind == Some(MediaKind::Image) {
                        let label = self.command_hint(
                            CommandId::ToggleReadingMode,
                            localization::Text::ReadingMode.in_language(language),
                        );
                        let enabled = self.reading_control_enabled();
                        let direction = self
                            .reading_drag
                            .as_ref()
                            .filter(|drag| !drag.was_enabled)
                            .and_then(|drag| drag.direction)
                            .or_else(|| {
                                self.reading_mode.then_some(self.reading_settings.reversed)
                            });
                        let response =
                            chrome::reading_button(ui, enabled, self.reading_mode, direction);
                        if let Some(drag) = &self.reading_drag {
                            // A pending spread may temporarily disable the button, but
                            // the pinned gesture still owns its cursor until release.
                            ui.ctx().set_cursor_icon(if drag.was_enabled {
                                egui::CursorIcon::Move
                            } else {
                                egui::CursorIcon::ResizeHorizontal
                            });
                        }
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                response.enabled(),
                                &label,
                            )
                        });
                        if response.drag_started_by(egui::PointerButton::Primary)
                            && ui.input(|input| {
                                input.pointer.primary_down() && input.modifiers.is_none()
                            })
                            && let Some((origin, position)) = ui.input(|input| {
                                input
                                    .pointer
                                    .press_origin()
                                    .zip(input.pointer.interact_pos())
                            })
                        {
                            actions.push(UiAction::BeginReadingDrag(origin, position - origin));
                        }
                        if response.clicked() {
                            actions.push(UiAction::Command(CommandId::ToggleReadingMode));
                        }
                        response
                            .help_ui(|ui| {
                                ui.label(label);
                                ui.label(localization::Text::ReadingDragHelp.in_language(language));
                            })
                            .disabled_help_text(
                                localization::Text::ReadingUnsavedHelp.in_language(language),
                            );
                        if let Some(position) = &info.image_position
                            && item(ui, RichText::new(position).size(chrome::TEXT_LG), true)
                                .help_text(info.folder_tooltip())
                                .clicked()
                        {
                            actions.push(UiAction::Command(CommandId::ToggleFilmstrip));
                        }
                    }
                    let full_info = info.tooltip();
                    let remaining = ui.available_width();
                    let budget = if self.active_export.is_some() {
                        (remaining * 0.52).min((remaining - 82.0).max(0.0))
                    } else {
                        remaining * 0.52
                    };
                    let mut fields = Vec::new();
                    let mut info_width = 0.0;
                    for (text, action) in info.fields.iter().zip(&info.actions) {
                        let field_width = width(ui, text);
                        let next = info_width
                            + field_width
                            + if fields.is_empty() {
                                0.0
                            } else {
                                chrome::ITEM_GAP
                            };
                        if next > budget && (!fields.is_empty() || next > remaining) {
                            break;
                        }
                        fields.push((text, action, field_width));
                        info_width = next;
                    }
                    let gap = if fields.is_empty() {
                        0.0
                    } else {
                        chrome::ITEM_GAP
                    };
                    ui.spacing_mut().item_spacing.x = gap;
                    let path_width = (remaining - info_width - gap).max(0.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(path_width, 24.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(path_width);
                            if let Some(export) = self
                                .active_export
                                .as_ref()
                                .filter(|_| self.export_error.is_none())
                            {
                                let (slot, _) = ui.allocate_exact_size(
                                    egui::vec2(path_width, 24.0),
                                    egui::Sense::hover(),
                                );
                                export_progress::show_status(
                                    ui.ctx(),
                                    slot,
                                    (ui.layer_id().order == egui::Order::Middle)
                                        .then_some(ui.layer_id()),
                                    export,
                                    self.export_gesture_hint(),
                                    !self.dialog_input_blocked(),
                                    actions,
                                );
                                return;
                            }
                            let selection_hint = (!self.modal_input_blocked()
                                && self.reading_drag.is_none())
                            .then(|| {
                                selection::focus_hint(ui.ctx()).or_else(|| {
                                    self.timeline_is_visible()
                                        .then(|| time_selection::focus_hint(ui.ctx()))
                                        .flatten()
                                })
                            })
                            .flatten();
                            let mut path_link = false;
                            let mut export_link = None;
                            let mut reading_adjustment = None;
                            let (text, color, tooltip) = if let Some(message) =
                                self.visual_selection_status().filter(|_| {
                                    matches!(
                                        self.view_drag,
                                        Some(
                                            ViewDrag::Selection { .. }
                                                | ViewDrag::MoveSelection { .. }
                                        )
                                    )
                                }) {
                                (message.clone(), chrome::FOREGROUND, message)
                            } else if let Some(message) = selection_hint {
                                (message.clone(), chrome::FOREGROUND, message)
                            } else if let Some(message) = self.status_notice() {
                                reading_adjustment = self.reading_adjustment_hint();
                                export_link =
                                    self.export_notice.as_ref().map(|(shown, _)| *shown).filter(
                                        |shown| self.export_notice_target(*shown).is_some(),
                                    );
                                (
                                    self.compact_export_notice(&message)
                                        .unwrap_or_else(|| message.clone()),
                                    chrome::FOREGROUND,
                                    message,
                                )
                            } else if let Some(path) = self.displayed_image_path() {
                                path_link =
                                    !self.current_source_deleted() && !self.update_is_held();
                                let parent = path
                                    .parent()
                                    .and_then(Path::file_name)
                                    .map(|name| name.to_string_lossy())
                                    .unwrap_or_default();
                                let mut suffix = self
                                    .visual_selection_status()
                                    .map_or_else(String::new, |value| format!(" · {value}"));
                                if let Some(notice) = self.folder_order_notice() {
                                    suffix.push_str(" · ");
                                    suffix.push_str(notice);
                                }
                                (
                                    format!(
                                        "{}{parent}\\{}{suffix}",
                                        self.deleted_path_prefix(),
                                        display_name(path)
                                    ),
                                    chrome::MUTED,
                                    format!(
                                        "{}{}{suffix}",
                                        self.deleted_path_prefix(),
                                        path.display()
                                    ),
                                )
                            } else if self.media_kind.is_none()
                                && !self.keyboard_settings_active()
                                && let Some(date) = self.gallery_listing.status_date(language)
                            {
                                (
                                    self.gallery_listing.status_name().map_or_else(
                                        || date.clone(),
                                        |name| format!("{date} · {name}"),
                                    ),
                                    chrome::MUTED,
                                    towavue_core::localization::formatted::last_opened(
                                        language, &date,
                                    ),
                                )
                            } else if self.current_document_untitled() {
                                let suffix = self
                                    .visual_selection_status()
                                    .map_or_else(String::new, |value| format!(" · {value}"));
                                (
                                    format!("{}{suffix}", image_paste::DEFAULT_NAME),
                                    chrome::MUTED,
                                    format!(
                                        "{}{suffix}",
                                        localization::Text::StatusPasted.in_language(language)
                                    ),
                                )
                            } else {
                                (
                                    if self.keyboard_settings_active() {
                                        localization::Text::ShortcutEditHint
                                            .in_language(language)
                                            .into()
                                    } else {
                                        localization::Text::OpenToBegin.in_language(language).into()
                                    },
                                    chrome::MUTED,
                                    towavue_core::localization::formatted::shortcuts_file(
                                        language,
                                        &self.shortcut_path.display().to_string(),
                                    ),
                                )
                            };
                            let reading_hint = self.reading_drag.is_some();
                            let label: egui::WidgetText = if let Some(hint) = &reading_adjustment {
                                hint.layout(12.0).into()
                            } else if reading_hint {
                                fonts::reading_hint(&text, 12.0, color).into()
                            } else if let Some(job) = self.hold_progress_label(&text) {
                                job.into()
                            } else {
                                RichText::new(text)
                                    .size(crate::chrome::TEXT_LG)
                                    .color(if path_link {
                                        Color32::PLACEHOLDER
                                    } else {
                                        color
                                    })
                                    .into()
                            };
                            let (slot, _) = ui.allocate_exact_size(
                                egui::vec2(path_width, 24.0),
                                egui::Sense::hover(),
                            );
                            let response =
                                item_at(ui, slot, label, path_link || export_link.is_some())
                                    .help_ui_above(|ui| {
                                        ui.set_max_width(ui.spacing().tooltip_width);
                                        if let Some(hint) = &reading_adjustment {
                                            ui.label(hint.layout(hover_help::TEXT_SIZE));
                                        } else if reading_hint {
                                            ui.label(fonts::reading_hint(
                                                &tooltip,
                                                hover_help::TEXT_SIZE,
                                                chrome::FOREGROUND,
                                            ));
                                        } else {
                                            let hint = if export_link.is_some_and(|shown| {
                                                self.export_notice_open_target(shown).is_some()
                                            }) {
                                                localization::Text::ExportLinkOpenHelp
                                                    .in_language(language)
                                            } else if export_link.is_some() {
                                                localization::Text::ExportLinkRevealHelp
                                                    .in_language(language)
                                            } else {
                                                ""
                                            };
                                            ui.label(format!("{tooltip}{hint}"));
                                        }
                                    });
                            if path_link && response.clicked() {
                                actions.push(UiAction::Command(CommandId::SearchCurrentFiles));
                            }
                            if path_link && let Some(path) = self.displayed_image_path() {
                                path_menu(
                                    ui,
                                    &response,
                                    path,
                                    self.tabs.active_id().map(|tab| thumbnail_menu::Owner {
                                        tab,
                                        instance: self.media_generation,
                                    }),
                                    actions,
                                );
                            }
                            if let Some(shown) = export_link {
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        response.enabled(),
                                        localization::Text::RevealExportedFile
                                            .in_language(language),
                                    )
                                });
                                if response.clicked() {
                                    actions.push(UiAction::RevealExport(shown));
                                }
                                if response.clicked_by(egui::PointerButton::Middle)
                                    && self.export_notice_open_target(shown).is_some()
                                {
                                    actions.push(UiAction::OpenExport(shown));
                                }
                                response.on_hover_cursor(egui::CursorIcon::PointingHand);
                            }
                        },
                    );
                    let info_width = (info_right - ui.next_widget_position().x).max(0.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(info_width, 24.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(info_width);
                            ui.spacing_mut().item_spacing.x = chrome::ITEM_GAP;
                            let mut right = info_right;
                            for (text, action, field_width) in fields.into_iter().rev() {
                                let rect = egui::Rect::from_min_size(
                                    egui::pos2(right - field_width, ui.max_rect().top()),
                                    egui::vec2(field_width, 24.0),
                                );
                                right -= field_width + chrome::ITEM_GAP;
                                let response = item_at(
                                    ui,
                                    rect,
                                    RichText::new(text).size(chrome::TEXT_LG),
                                    action.is_some(),
                                )
                                .help_text(&full_info);
                                if response.clicked()
                                    && let Some(action) = action
                                {
                                    actions.push(match action {
                                        status_info::Action::Command(command) => {
                                            UiAction::Command(*command)
                                        }
                                        status_info::Action::Palette(query) => {
                                            UiAction::StatusPalette(query)
                                        }
                                    });
                                }
                            }
                        },
                    );
                });
            })
            .response
            .rect
    }
}
