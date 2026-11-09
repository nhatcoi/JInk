use gpui::*;
use crate::theme::Theme;
use crate::settings::{load_settings, save_settings, Settings};
use crate::ai::{self, AiConfig, AiStreamEvent};
use crate::inject;
use crate::text_input::{TextInput, TextInputEvent};

#[derive(PartialEq, Clone, Copy)]
pub enum ActiveView {
    Input,
    Settings,
}

pub struct JInkPopup {
    pub input: Entity<TextInput>,
    _input_sub: Subscription,
    pub url_input: Entity<TextInput>,
    pub model_input: Entity<TextInput>,
    pub hotkey_input: Entity<TextInput>,
    pub key_input: Entity<TextInput>,
    _field_subs: Vec<Subscription>,
    pub undo_text: Option<String>,
    pub status: Option<String>,
    pub is_streaming: bool,
    pub settings: Settings,
    pub active_view: ActiveView,
    pub focus_handle: FocusHandle,
}

impl JInkPopup {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = load_settings();
        let input = cx.new(|cx| TextInput::new("Type anything…", cx));
        let sub = cx.subscribe_in(&input, window, |this, _, event: &TextInputEvent, window, cx| {
            match event {
                TextInputEvent::Submit => this.trigger_inject(window, cx),
                TextInputEvent::UndoEmpty => this.undo_ai(cx),
                TextInputEvent::Changed => {}
            }
        });
        let url_input = cx.new(|cx| TextInput::new_field("https://api.openai.com/v1", &s.ai_base_url, 330.0, cx));
        let model_input = cx.new(|cx| TextInput::new_field("gpt-4o-mini", &s.ai_model, 330.0, cx));
        let hotkey_input = cx.new(|cx| TextInput::new_field("Control+Cmd+Space", &s.hotkey, 330.0, cx));
        let key_input = cx.new(|cx| TextInput::new_field("sk-…", &s.ai_key, 330.0, cx).masked());
        let mut field_subs = Vec::new();
        field_subs.push(cx.subscribe(&key_input, |this, e, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Changed = ev {
                this.settings.ai_key = e.read(cx).text().trim().to_string();
                let _ = save_settings(&this.settings);
            }
        }));
        field_subs.push(cx.subscribe(&url_input, |this, e, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Changed = ev {
                this.settings.ai_base_url = e.read(cx).text().trim().to_string();
                let _ = save_settings(&this.settings);
            }
        }));
        field_subs.push(cx.subscribe(&model_input, |this, e, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Changed = ev {
                this.settings.ai_model = e.read(cx).text().trim().to_string();
                let _ = save_settings(&this.settings);
            }
        }));
        field_subs.push(cx.subscribe(&hotkey_input, |this, e, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Changed = ev {
                this.settings.hotkey = e.read(cx).text().trim().to_string();
                let _ = save_settings(&this.settings);
            }
        }));
        Self {
            input,
            _input_sub: sub,
            url_input,
            model_input,
            hotkey_input,
            key_input,
            _field_subs: field_subs,
            undo_text: None,
            status: None,
            is_streaming: false,
            settings: s,
            active_view: ActiveView::Input,
            focus_handle: cx.focus_handle(),
        }
    }

    fn current_text(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }

    pub fn trigger_inject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let to_inject = self.current_text(cx);
        window.remove_window();

        std::thread::spawn(move || {
            let _ = inject::inject_text(&to_inject, &[]);
        });
    }

    pub fn trigger_ai_enhance(&mut self, cx: &mut Context<Self>) {
        let text = self.current_text(cx);
        if text.trim().is_empty() || self.is_streaming {
            return;
        }
        let prompt_messages = ai::enhance_prompt(&text);
        self.start_ai_stream(prompt_messages, cx);
    }

    pub fn trigger_ai_translate(&mut self, cx: &mut Context<Self>) {
        let text = self.current_text(cx);
        if text.trim().is_empty() || self.is_streaming {
            return;
        }
        let prompt_messages = ai::translate_prompt(
            &text,
            &self.settings.translate_from,
            &self.settings.translate_to,
        );
        self.start_ai_stream(prompt_messages, cx);
    }

    pub fn trigger_ai_explain(&mut self, cx: &mut Context<Self>) {
        let text = self.current_text(cx);
        if text.trim().is_empty() || self.is_streaming {
            return;
        }
        let prompt_messages = ai::explain_prompt(&text, &self.settings.main_language);
        self.start_ai_stream(prompt_messages, cx);
    }

    pub fn undo_ai(&mut self, cx: &mut Context<Self>) {
        if let Some(prev) = self.undo_text.take() {
            self.input.update(cx, |i, cx| i.set_text_silent(&prev, cx));
            self.status = Some("Undone".into());
            cx.notify();
        }
    }

    pub fn save_current_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = save_settings(&self.settings) {
            self.status = Some(format!("Could not save settings: {e}"));
        } else {
            self.status = Some("Settings saved".into());
        }
        cx.notify();
    }

    #[allow(dead_code)]
    pub fn paste_api_key_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(clip_text) = cb.get_text() {
                let trimmed = clip_text.trim().to_string();
                if !trimmed.is_empty() {
                    self.settings.ai_key = trimmed.clone();
                    self.key_input.update(cx, |i, cx| i.set_text_silent(&trimmed, cx));
                    self.save_current_settings(cx);
                    self.status = Some("API key updated from clipboard".into());
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
            self.status = Some("No API key yet. Add one in settings.".into());
            self.active_view = ActiveView::Settings;
            cx.notify();
            return;
        }

        self.undo_text = Some(self.current_text(cx));
        self.input.update(cx, |i, cx| i.set_text_silent("", cx));
        self.is_streaming = true;
        self.status = Some("Writing…".into());
        cx.notify();

        let config = AiConfig {
            base_url: self.settings.ai_base_url.clone(),
            model: self.settings.ai_model.clone(),
            api_key: self.settings.ai_key.clone(),
            search_key: self.settings.search_key.clone(),
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        // 1. Chạy network call trên Background thread pool (không block UI/Main thread)
        crate::runtime().spawn(async move {
            ai::stream_chat(config, messages, vec![], tx).await;
        });

        // 2. Lắng nghe token trên UI context để update mượt mà
        cx.spawn(async move |entity, cx| {
            while let Some(event) = rx.recv().await {
                let _ = entity.update(cx, |this, cx| match event {
                    AiStreamEvent::Token(token) => {
                        this.input.update(cx, |i, cx| i.append_silent(&token, cx));
                        cx.notify();
                    }
                    AiStreamEvent::Done => {
                        this.is_streaming = false;
                        this.status = None;
                        let cleaned = ai::strip_think_tags(this.input.read(cx).text());
                        this.input.update(cx, |i, cx| i.set_text_silent(&cleaned, cx));
                        cx.notify();
                    }
                    AiStreamEvent::Error(err) => {
                        this.is_streaming = false;
                        this.status = Some(format!("Error: {err}"));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
}

impl Render for JInkPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_settings = self.active_view == ActiveView::Settings;

        // Keep keyboard focus on a rendered element so Esc / typing always work.
        let input_focus = self.input.read(cx).focus_handle(cx);
        if is_settings {
            let field_focused = self.url_input.read(cx).is_focused(window)
                || self.model_input.read(cx).is_focused(window)
                || self.hotkey_input.read(cx).is_focused(window)
                || self.key_input.read(cx).is_focused(window);
            if !field_focused && !self.focus_handle.is_focused(window) {
                window.focus(&self.focus_handle);
            }
        } else if self.focus_handle.is_focused(window) {
            window.focus(&input_focus);
        }

        // Window grows for the settings panel, shrinks back for the input.
        let want_h = if is_settings {
            340.0
        } else {
            96.0 + self.input.read(cx).visible_height()
        };
        if (window.viewport_size().height - px(want_h)).abs() > px(1.0) {
            window.resize(size(px(560.0), px(want_h)));
        }

        let pill = div()
            .track_focus(&self.focus_handle)
            .key_context("JInkPopup")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let mods = &event.keystroke.modifiers;

                if key == "escape" {
                    if this.active_view == ActiveView::Settings {
                        this.active_view = ActiveView::Input;
                        let h = this.input.read(cx).focus_handle(cx);
                        window.focus(&h);
                        cx.notify();
                    } else {
                        window.remove_window();
                    }
                    return;
                }

                if this.active_view == ActiveView::Input && (mods.platform || mods.control) {
                    match key {
                        "e" => this.trigger_ai_enhance(cx),
                        "t" => this.trigger_ai_translate(cx),
                        "g" => this.trigger_ai_explain(cx),
                        _ => {}
                    }
                }
            }))
            .flex()
            .flex_row()
            .size_full()
            .font_family("Menlo")
            .bg(Theme::pill_bg())
            .border_1()
            .border_color(Theme::glass_edge())
            .overflow_hidden()
            // vermilion spine
            .child(div().w(px(3.0)).flex_none().bg(Theme::seal()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .child(if is_settings {
                        self.render_settings_view(cx).into_any_element()
                    } else {
                        self.render_input_view(cx).into_any_element()
                    }),
            );

        pill
    }
}

impl JInkPopup {
    /// Translate target as two tiny tabs: active one is paper-colored with a seal underline.
    fn render_lang_toggle(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.settings.translate_to.clone();
        let seg = |code: &'static str, label: &'static str, cx: &mut Context<Self>| {
            let active = current == code;
            div()
                .px_1p5()
                .pb(px(1.0))
                .text_xs()
                .border_b_1()
                .border_color(if active { Theme::seal() } else { gpui::rgba(0x00000000) })
                .text_color(if active { Theme::paper() } else { Theme::hint() })
                .hover(|st| st.cursor_pointer().text_color(Theme::paper()))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                    this.settings.translate_to = code.to_string();
                    let _ = crate::settings::save_settings(&this.settings);
                    cx.notify();
                }))
                .child(label)
        };
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(div().text_xs().text_color(Theme::hint()).child("translate →"))
            .child(seg("en", "EN", cx))
            .child(seg("vi", "VI", cx))
    }

    fn render_input_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_text = !self.input.read(cx).is_blank();
        let is_busy = self.is_streaming;

        div()
            .flex()
            .flex_col()
            .gap_2()
            // header: wordmark + status + settings
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(Theme::seal())
                                    .child("JINK"),
                            )
                            .child(if let Some(status) = &self.status {
                                div()
                                    .text_xs()
                                    .text_color(Theme::amber())
                                    .child(status.clone())
                                    .into_any_element()
                            } else {
                                div()
                                    .text_xs()
                                    .text_color(Theme::hint())
                                    .child("write → refine → insert")
                                    .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(self.render_lang_toggle(cx))
                            .child(
                                div()
                                    .p_1()
                                    .rounded(px(4.0))
                                    .hover(|st| st.bg(Theme::chip_hover()).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.active_view = ActiveView::Settings;
                                        cx.notify();
                                    }))
                                    .child(Self::icon("settings", 14.0, Theme::hint())),
                            ),
                    ),
            )
            // prompt + input
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .w(px(10.0))
                            .flex_none()
                            .text_size(px(14.0))
                            .line_height(px(22.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::seal())
                            .child("›"),
                    )
                    .child(div().flex_1().child(self.input.clone())),
            )
            // hairline
            .child(div().h(px(1.0)).w_full().bg(Theme::pill_border()))
            // footer: keycap actions · lang · stamp
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.action_button("⌘E", "refine", is_busy, cx.listener(|this, _, _, cx| {
                                this.trigger_ai_enhance(cx);
                            })))
                            .child(self.action_button("⌘T", "translate", is_busy, cx.listener(|this, _, _, cx| {
                                this.trigger_ai_translate(cx);
                            })))
                            .child(self.action_button("⌘G", "explain", is_busy, cx.listener(|this, _, _, cx| {
                                this.trigger_ai_explain(cx);
                            })))
                            .child(if self.undo_text.is_some() {
                                self.action_button("⌘Z", "undo", false, cx.listener(|this, _, _, cx| {
                                    this.undo_ai(cx);
                                })).into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .bg(if has_text { Theme::seal() } else { Theme::chip_hover() })
                                    .text_color(if has_text { Theme::ink() } else { Theme::hint() })
                                    .hover(|st| st.opacity(0.88).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                                        this.trigger_inject(window, cx);
                                    }))
                                    .child("INSERT ↵"),
                            ),
                    ),
            )
    }

    fn render_settings_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let chip = |label: &'static str| {
            div()
                .px_2()
                .py_1()
                .rounded(px(4.0))
                .text_xs()
                .bg(Theme::chip_hover())
                .border_1()
                .border_color(Theme::pill_border())
                .text_color(Theme::paper())
                .hover(|st| st.text_color(Theme::seal()).cursor_pointer())
                .child(label)
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::seal())
                            .child("JINK · SETTINGS"),
                    )
                    .child(
                        chip("esc · back").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.active_view = ActiveView::Input;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(div().h(px(1.0)).w_full().bg(Theme::pill_border()))
            .child(Self::field_row("Base URL", self.url_input.clone().into_any_element()))
            .child(Self::field_row("Model", self.model_input.clone().into_any_element()))
            .child(Self::field_row("API Key", self.key_input.clone().into_any_element()))
            .child(Self::field_row("Hotkey", self.hotkey_input.clone().into_any_element()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pt_1()
                    .child(div().text_xs().text_color(Theme::hint()).child("preset"))
                    .child(chip("OpenAI").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.set_preset("https://api.openai.com/v1", "gpt-4o-mini", cx);
                            this.sync_fields(cx);
                        }),
                    ))
                    .child(chip("Ollama").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.set_preset("http://localhost:11434/v1", "qwen2.5:7b", cx);
                            this.sync_fields(cx);
                        }),
                    ))
                    .child(chip("OpenRouter").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.set_preset(
                                "https://openrouter.ai/api/v1",
                                "meta-llama/llama-3.3-70b-instruct",
                                cx,
                            );
                            this.sync_fields(cx);
                        }),
                    )),
            )
            .child(div().h(px(1.0)).w_full().bg(Theme::pill_border()))
            .child(if let Some(status) = &self.status {
                div().text_xs().text_color(Theme::amber()).child(status.clone()).into_any_element()
            } else {
                div()
                    .text_xs()
                    .text_color(Theme::hint())
                    .child("~/.config/jink/settings.json · restart to apply hotkey")
                    .into_any_element()
            })
    }

    fn sync_fields(&mut self, cx: &mut Context<Self>) {
        let (u, m) = (self.settings.ai_base_url.clone(), self.settings.ai_model.clone());
        self.url_input.update(cx, |i, cx| i.set_text_silent(&u, cx));
        self.model_input.update(cx, |i, cx| i.set_text_silent(&m, cx));
    }

    fn field_row(label: &'static str, field: AnyElement) -> impl IntoElement {
        div()
            .flex()
            .items_start()
            .gap_3()
            .child(
                div()
                    .w(px(70.0))
                    .flex_none()
                    .pt(px(5.0))
                    .text_xs()
                    .text_color(Theme::hint())
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .px_2()
                    .py(px(3.0))
                    .rounded(px(4.0))
                    .bg(Theme::chip_hover())
                    .border_1()
                    .border_color(Theme::pill_border())
                    .child(field),
            )
    }

    fn icon(name: &'static str, size: f32, color: Rgba) -> impl IntoElement {
        svg()
            .path(format!("svg/{name}.svg"))
            .size(px(size))
            .text_color(color)
            .flex_none()
    }

    fn action_button(
        &self,
        key: &'static str,
        label: &'static str,
        is_disabled: bool,
        on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let fg = if is_disabled { Theme::hint() } else { Theme::paper() };
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .text_xs()
            .text_color(fg)
            .hover(|st| if is_disabled { st } else { st.text_color(Theme::seal()).cursor_pointer() })
            .on_mouse_down(MouseButton::Left, on_click)
            .child(
                div()
                    .px_1()
                    .rounded(px(3.0))
                    .bg(Theme::chip_hover())
                    .border_1()
                    .border_color(Theme::pill_border())
                    .text_color(Theme::hint())
                    .child(key),
            )
            .child(label)
    }
}
