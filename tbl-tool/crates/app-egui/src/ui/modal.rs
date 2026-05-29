use eframe::egui;

/// 模态遮罩：在指定层级（low）画一张全屏半透明蒙版，吸收点击。
/// 返回值表示用户是否点击了遮罩区域（即"点击外部"），调用方应据此关闭弹窗。
pub fn modal_scrim(ctx: &egui::Context, id_salt: &str) -> bool {
    let screen = ctx.screen_rect();
    let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new(("modal_scrim", id_salt)));
    let painter = ctx.layer_painter(layer.clone());
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(96));

    let resp = egui::Area::new(egui::Id::new(("modal_scrim_area", id_salt)))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            ui.allocate_rect(screen, egui::Sense::click_and_drag())
        });
    resp.inner.clicked()
}

/// 在弹窗底部画一行右对齐的按钮（Windows 习惯：主操作 → 取消，取消最右）。
/// 内部用 `min_rect().width()` 拿到已布局内容的实际宽度，避免 `available_width()`
/// 在 `Window::resizable(false)` 下返回不准导致按钮贴左。
pub fn dialog_buttons<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let w = ui.min_rect().width().max(180.0);
    ui.allocate_ui_with_layout(
        egui::vec2(w, 28.0),
        egui::Layout::right_to_left(egui::Align::Center),
        content,
    ).inner
}
