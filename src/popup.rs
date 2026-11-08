use gpui::*;
use crate::theme::Theme;
use crate::settings::{load_settings, save_settings, Settings};
use crate::ai::{self, AiConfig, AiStreamEvent};
use crate::inject;

#[derive(PartialEq, Clone, Copy)]
pub enum ActiveView {
    Input,
    Settings,
}

pub struct JInkPopup {
    pub text: String,
    pub cursor: usize,
    pub undo_text: Option<String>,
    pub status: Option<String>,
    pub is_streaming: bool,
    pub settings: Settings,
    pub active_view: ActiveView,
    pub focus_handle: FocusHandle,
}

impl JInkPopup {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let s = load_settings();
        Self {
            text: String::new(),
            cursor: 0,
            undo_text: None,
            status: None,
            is_streaming: false,
            settings: s,
            active_view: ActiveView::Input,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn insert_text(&mut self, s: &str, cx: &mut Context<Self>) {
        if self.cursor >= self.text.len() {
            self.text.push_str(s);
            self.cursor = self.text.len();
        } else {
            self.text.insert_str(self.cursor, s);
            self.cursor += s.len();
        }
        cx.notify();
    }

    pub fn backspace(&mut self, cx: &mut Context<Self>) {
        if self.cursor > 0 && !self.text.is_empty() {
            let mut prev_char_idx = self.cursor - 1;
            while !self.text.is_char_boundary(prev_char_idx) && prev_char_idx > 0 {
                prev_char_idx -= 1;
            }
            self.text.remove(prev_char_idx);
            self.cursor = prev_char_idx;
            cx.notify();
        }
    }

    pub fn delete_forward(&mut self, cx: &mut Context<Self>) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
            cx.notify();
        }
    }

    pub fn move_cursor_left(&mut self, cx: &mut Context<Self>) {
        if self.cursor > 0 {
            let mut prev = self.cursor - 1;
            while !self.text.is_char_boundary(prev) && prev > 0 {
                prev -= 1;
            }
            self.cursor = prev;
            cx.notify();
        }
    }

    pub fn move_cursor_right(&mut self, cx: &mut Context<Self>) {
        if self.cursor < self.text.len() {
            let mut next = self.cursor + 1;
            while !self.text.is_char_boundary(next) && next < self.text.len() {
                next += 1;
            }
            self.cursor = next;
            cx.notify();
        }
    }

    pub fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(clip_text) = cb.get_text() {
                self.insert_text(&clip_text, cx);
            }
        }
    }

    pub fn copy_clipboard(&mut self) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(self.text.clone());
        }
    }

    pub fn trigger_inject(&mut self, window: &mut Window) {
        let to_inject = self.text.clone();
        window.remove_window();

        std::thread::spawn(move || {
            let _ = inject::inject_text(&to_inject, &[]);
        });
    }

    pub fn trigger_ai_enhance(&mut self, cx: &mut Context<Self>) {
        if self.text.trim().is_empty() || self.is_streaming {
            return;
        }
        let prompt_messages = ai::enhance_prompt(&self.text);
        self.start_ai_stream(prompt_messages, cx);
    }

    pub fn trigger_ai_translate(&mut self, cx: &mut Context<Self>) {
        if self.text.trim().is_empty() || self.is_streaming {
            return;
        }
        let prompt_messages = ai::translate_prompt(
            &self.text,
            &self.settings.translate_from,
            &self.settings.translate_to,
        );
        self.start_ai_stream(prompt_messages, cx);
    }

    pub fn trigger_ai_explain(&mut self, cx: &mut Context<Self>) {
        if self.text.trim().is_empty() || self.is_streaming {
            return;
        }
        let prompt_messages = ai::explain_prompt(&self.text, &self.settings.main_language);
        self.start_ai_stream(prompt_messages, cx);
    }

    pub fn undo_ai(&mut self, cx: &mut Context<Self>) {
        if let Some(prev) = self.undo_text.take() {
            self.text = prev;
            self.cursor = self.text.len();
            self.status = Some("Đã hoàn tác".into());
            cx.notify();
        }
    }

    pub fn save_current_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = save_settings(&self.settings) {
            self.status = Some(format!("Lỗi lưu cài đặt: {e}"));
        } else {
            self.status = Some("Đã lưu cài đặt".into());
        }
        cx.notify();
    }

    pub fn paste_api_key_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(clip_text) = cb.get_text() {
                let trimmed = clip_text.trim().to_string();
                if !trimmed.is_empty() {
                    self.settings.ai_key = trimmed;
                    self.save_current_settings(cx);
                    self.status = Some("Đã cập nhật API Key từ Clipboard!".into());
                    cx.notify();
                }
            }
        }
    }

    pub fn set_preset(&mut self, base_url: &str, model: &str, cx: &mut Context<Self>) {
        self.settings.ai_base_url = base_url.to_string();
        self.settings.ai_model = model.to_string();
        self.save_current_settings(cx);
    }

    fn start_ai_stream(&mut self, messages: Vec<ai::ChatMessage>, cx: &mut Context<Self>) {
        let is_local = self.settings.ai_base_url.contains("localhost") || self.settings.ai_base_url.contains("127.0.0.1");
        if self.settings.ai_key.trim().is_empty() && !is_local {
            self.status = Some("Chưa có API Key. Bấm ⚙️ để nhập key.".into());
            self.active_view = ActiveView::Settings;
            cx.notify();
            return;
        }

        self.undo_text = Some(self.text.clone());
        self.text.clear();
        self.cursor = 0;
        self.is_streaming = true;
        self.status = Some("AI đang soạn thảo...".into());
        cx.notify();

        let config = AiConfig {
            base_url: self.settings.ai_base_url.clone(),
            model: self.settings.ai_model.clone(),
            api_key: self.settings.ai_key.clone(),
            search_key: self.settings.search_key.clone(),
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        // 1. Chạy network call trên Background thread pool (không block UI/Main thread)
        cx.background_executor()
            .spawn(async move {
                ai::stream_chat(config, messages, vec![], tx).await;
            })
            .detach();

        // 2. Lắng nghe token trên UI context để update mượt mà
        cx.spawn(async move |entity, cx| {
            while let Some(event) = rx.recv().await {
                let _ = entity.update(cx, |this, cx| match event {
                    AiStreamEvent::Token(token) => {
                        this.text.push_str(&token);
                        this.cursor = this.text.len();
                        cx.notify();
                    }
                    AiStreamEvent::Done => {
                        this.is_streaming = false;
                        this.status = None;
                        this.text = ai::strip_think_tags(&this.text);
                        this.cursor = this.text.len();
                        cx.notify();
                    }
                    AiStreamEvent::Error(err) => {
                        this.is_streaming = false;
                        this.status = Some(format!("Lỗi: {err}"));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
}

impl Render for JInkPopup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_settings = self.active_view == ActiveView::Settings;

        div()
            .track_focus(&self.focus_handle)
            .key_context("JInkPopup")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let mods = &event.keystroke.modifiers;

                // Handle global shortcuts
                if key == "escape" {
                    if this.active_view == ActiveView::Settings {
                        this.active_view = ActiveView::Input;
                        cx.notify();
                    } else {
                        window.remove_window();
                    }
                    return;
                }

                if this.active_view == ActiveView::Input {
                    // Enter -> Inject
                    if key == "enter" && !mods.shift && !mods.alt {
                        this.trigger_inject(window);
                        return;
                    }
                    // Shift+Enter -> Newline
                    if key == "enter" && mods.shift {
                        this.insert_text("\n", cx);
                        return;
                    }
                    // Cmd+V / Ctrl+V
                    if (mods.platform || mods.control) && key == "v" {
                        this.paste_clipboard(cx);
                        return;
                    }
                    // Cmd+C / Ctrl+C
                    if (mods.platform || mods.control) && key == "c" {
                        this.copy_clipboard();
                        return;
                    }
                    // Cmd+Z -> Undo
                    if (mods.platform || mods.control) && key == "z" {
                        this.undo_ai(cx);
                        return;
                    }
                    // Cmd+E -> AI Enhance
                    if (mods.platform || mods.control) && key == "e" {
                        this.trigger_ai_enhance(cx);
                        return;
                    }
                    // Cmd+T -> AI Translate
                    if (mods.platform || mods.control) && key == "t" {
                        this.trigger_ai_translate(cx);
                        return;
                    }

                    // Navigation & Editing
                    match key {
                        "backspace" => this.backspace(cx),
                        "delete" => this.delete_forward(cx),
                        "left" => this.move_cursor_left(cx),
                        "right" => this.move_cursor_right(cx),
                        "home" => {
                            this.cursor = 0;
                            cx.notify();
                        }
                        "end" => {
                            this.cursor = this.text.len();
                            cx.notify();
                        }
                        _ => {
                            if let Some(char_str) = &event.keystroke.key_char {
                                if !mods.platform && !mods.control {
                                    this.insert_text(char_str, cx);
                                }
                            }
                        }
                    }
                }
            }))
            .flex()
            .flex_col()
            .w(px(680.0))
            .bg(Theme::mantle())
            .rounded(px(16.0))
            .border_1()
            .border_color(Theme::surface0())
            .shadow_2xl()
            .p_4()
            .gap_3()
            .child(if is_settings {
                self.render_settings_view(cx).into_any_element()
            } else {
                self.render_input_view(cx).into_any_element()
            })
    }
}

impl JInkPopup {
    fn render_input_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let (before_cursor, after_cursor) = self.text.split_at(self.cursor.min(self.text.len()));
        let has_text = !self.text.trim().is_empty();
        let is_busy = self.is_streaming;

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                // Text Area Container
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .w_full()
                    .min_h(px(72.0))
                    .max_h(px(260.0))
                    .p_3()
                    .bg(Theme::base())
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(Theme::surface0())
                    .cursor_text()
                    .child(if self.text.is_empty() {
                        div()
                            .text_sm()
                            .text_color(Theme::overlay0())
                            .child("Nhập nội dung cần hỗ trợ hoặc dùng phím tắt AI...")
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .flex_wrap()
                            .text_sm()
                            .text_color(Theme::text())
                            .child(before_cursor.to_string())
                            // Blinking / crisp vertical cursor bar
                            .child(
                                div()
                                    .w(px(2.0))
                                    .h(px(16.0))
                                    .bg(Theme::blue())
                                    .rounded_full()
                                    .mx(px(0.5)),
                            )
                            .child(after_cursor.to_string())
                            .into_any_element()
                    }),
            )
            .child(
                // Bottom Bar: Actions & Status
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt_1()
                    // Left: AI Action buttons
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                self.action_button("✨ Enhance (Cmd+E)", is_busy, cx.listener(|this, _, _, cx| {
                                    this.trigger_ai_enhance(cx);
                                }))
                            )
                            .child(
                                self.action_button("🌐 Dịch (Cmd+T)", is_busy, cx.listener(|this, _, _, cx| {
                                    this.trigger_ai_translate(cx);
                                }))
                            )
                            .child(
                                self.action_button("📖 Giải thích", is_busy, cx.listener(|this, _, _, cx| {
                                    this.trigger_ai_explain(cx);
                                }))
                            )
                            .child(if self.undo_text.is_some() {
                                self.action_button("↩ Hoàn tác", false, cx.listener(|this, _, _, cx| {
                                    this.undo_ai(cx);
                                })).into_any_element()
                            } else {
                                div().into_any_element()
                            })
                    )
                    // Right: Inject, Settings, Status
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(if let Some(status) = &self.status {
                                div()
                                    .text_xs()
                                    .text_color(Theme::yellow())
                                    .child(status.clone())
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            })
                            .child(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .bg(if has_text { Theme::blue() } else { Theme::surface0() })
                                    .text_color(if has_text { Theme::crust() } else { Theme::overlay0() })
                                    .hover(|style| style.opacity(0.9).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, _| {
                                        this.trigger_inject(window);
                                    }))
                                    .child("↵ Inject")
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .bg(Theme::surface0())
                                    .text_color(Theme::subtext0())
                                    .hover(|style| style.bg(Theme::surface1()).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.active_view = ActiveView::Settings;
                                        cx.notify();
                                    }))
                                    .child("⚙️")
                            )
                    )
            )
    }

    fn render_settings_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_key = !self.settings.ai_key.trim().is_empty();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(Theme::surface0())
                    .pb_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::text())
                            .child("⚙️ Cài đặt JInk Native"),
                    )
                    .child(
                        div()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .text_xs()
                            .bg(Theme::surface0())
                            .text_color(Theme::text())
                            .hover(|s| s.bg(Theme::surface1()).cursor_pointer())
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.active_view = ActiveView::Input;
                                cx.notify();
                            }))
                            .child("✕ Quay lại"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.render_setting_row("AI Base URL", &self.settings.ai_base_url, "https://api.openai.com/v1"))
                    .child(self.render_setting_row("AI Model", &self.settings.ai_model, "gpt-4o-mini"))
                    .child(self.render_setting_row(
                        "API Key",
                        if has_key { "•••••••••••• (Đã cấu hình)" } else { "Chưa đặt (Nhấn nút dưới để dán)" },
                        "sk-..."
                    ))
                    .child(self.render_setting_row("Hotkey", &self.settings.hotkey, "Control+Cmd+Space"))
            )
            // Presets & Quick Actions
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pt_1()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(Theme::subtext0())
                            .child("Cấu hình nhanh:")
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .bg(Theme::surface0())
                                    .text_color(Theme::text())
                                    .hover(|s| s.bg(Theme::surface1()).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.set_preset("https://api.openai.com/v1", "gpt-4o-mini", cx);
                                    }))
                                    .child("OpenAI (gpt-4o-mini)")
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .bg(Theme::surface0())
                                    .text_color(Theme::text())
                                    .hover(|s| s.bg(Theme::surface1()).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.set_preset("http://localhost:11434/v1", "qwen2.5:7b", cx);
                                    }))
                                    .child("Ollama Local")
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .text_xs()
                                    .bg(Theme::surface0())
                                    .text_color(Theme::text())
                                    .hover(|s| s.bg(Theme::surface1()).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.set_preset("https://openrouter.ai/api/v1", "meta-llama/llama-3.3-70b-instruct", cx);
                                    }))
                                    .child("OpenRouter")
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pt_2()
                            .child(
                                div()
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .bg(Theme::blue())
                                    .text_color(Theme::crust())
                                    .hover(|s| s.opacity(0.9).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.paste_api_key_from_clipboard(cx);
                                    }))
                                    .child("📋 Dán API Key từ Clipboard")
                            )
                    )
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt_2()
                    .border_t_1()
                    .border_color(Theme::surface0())
                    .child(if let Some(status) = &self.status {
                        div().text_xs().text_color(Theme::yellow()).child(status.clone()).into_any_element()
                    } else {
                        div().text_xs().text_color(Theme::overlay0()).child("Tệp cấu hình: ~/.config/jink/settings.json").into_any_element()
                    })
            )
    }

    fn render_setting_row(&self, label: &'static str, value: &str, placeholder: &'static str) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .py_1()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Theme::subtext0())
                    .child(label),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(Theme::base())
                    .border_1()
                    .border_color(Theme::surface0())
                    .text_xs()
                    .text_color(if value.is_empty() { Theme::overlay0() } else { Theme::text() })
                    .child(if value.is_empty() { placeholder.to_string() } else { value.to_string() }),
            )
    }

    fn action_button(
        &self,
        label: &'static str,
        is_disabled: bool,
        on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .px_2p5()
            .py_1()
            .rounded_md()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .bg(Theme::surface0())
            .text_color(if is_disabled { Theme::overlay0() } else { Theme::text() })
            .hover(|style| {
                if !is_disabled {
                    style.bg(Theme::surface1()).cursor_pointer()
                } else {
                    style
                }
            })
            .on_mouse_down(MouseButton::Left, on_click)
            .child(label)
    }
}
