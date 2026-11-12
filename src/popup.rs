use gpui::*;
use crate::i18n::tr;
use crate::theme::Theme;
use crate::settings::{load_settings, save_settings, Settings};
use crate::ai::{self, AiConfig, AiStreamEvent};
use crate::inject;
use crate::text_input::{TextInput, TextInputEvent};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Attachment {
    name: String,
    path: String,
    /// N of the `[#ImageN]` token for images; None for plain files.
    image_index: Option<u32>,
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

#[cfg(target_os = "macos")]
const KEY_MOD: &str = "⌘";
#[cfg(not(target_os = "macos"))]
const KEY_MOD: &str = "^";

#[cfg(target_os = "macos")]
const FONT: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
const FONT: &str = "monospace";

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
    pub stt_input: Entity<TextInput>,
    pub hotkey_input: Entity<TextInput>,
    pub key_input: Entity<TextInput>,
    _field_subs: Vec<Subscription>,
    pub undo_text: Option<String>,
    /// Translate/explain show the result under the untouched original instead of replacing it.
    translation: Option<String>,
    recording: Option<crate::mic::Recording>,
    wave_bars: [f32; 10],
    pub status: Option<String>,
    pub is_streaming: bool,
    pub is_recording_hotkey: bool,
    pub settings: Settings,
    pub active_view: ActiveView,
    pub focus_handle: FocusHandle,
    attachments: Vec<Attachment>,
    // selection from another app: sent to explain/enhance as background only, never injected
    context: Option<String>,
    image_seq: u32,
    // our own file dialog steals focus; that must not count as click-outside
    picking_files: bool,
    // the WM drops window activation during a drag-move; that must not count as click-outside
    moving_since: Option<Instant>,
    last_origin: Point<Pixels>,
    still_polls: u8,
}

impl JInkPopup {
    /// Attach a selection that arrived after the popup opened (slow capture path).
    /// Prefill never overwrites anything the user already typed.
    pub fn attach_selection(&mut self, text: String, prefill: bool, cx: &mut Context<Self>) {
        if prefill {
            if self.input.read(cx).is_blank() {
                self.input.update(cx, |i, cx| i.set_text_silent(&text, cx));
            }
        } else {
            self.context = Some(text);
        }
        cx.notify();
    }

    pub fn new(initial_text: Option<String>, context: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = load_settings();
        let input = cx.new(|cx| TextInput::new("Type anything…", cx));
        if let Some(text) = initial_text {
            input.update(cx, |i, cx| i.set_text_silent(&text, cx));
        }
        Self::watch_click_outside(window, cx);
        let sub = cx.subscribe_in(&input, window, |this, _, event: &TextInputEvent, window, cx| {
            match event {
                TextInputEvent::Submit => this.trigger_inject(window, cx),
                TextInputEvent::UndoEmpty => this.undo_ai(cx),
                TextInputEvent::PasteImage => this.paste_clipboard_image(window, cx),
                TextInputEvent::Changed => {
                    if this.translation.is_some() && !this.is_streaming {
                        this.translation = None;
                    }
                }
            }
        });
        let url_input = cx.new(|cx| TextInput::new_field("https://api.openai.com/v1", &s.ai_base_url, 500.0, cx));
        let model_input = cx.new(|cx| TextInput::new_field("gpt-4o-mini", &s.ai_model, 500.0, cx));
        let stt_input = cx.new(|cx| TextInput::new_field("whisper-1", &s.stt_model, 500.0, cx));
        let default_hotkey_placeholder = crate::settings::HOTKEY_PRESETS[0].1;

        let hotkey_input = cx.new(|cx| TextInput::new_field(default_hotkey_placeholder, &s.hotkey, 500.0, cx));
        let key_input = cx.new(|cx| TextInput::new_field("sk-…", &s.ai_key, 500.0, cx).masked());
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
        field_subs.push(cx.subscribe(&stt_input, |this, e, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Changed = ev {
                this.settings.stt_model = e.read(cx).text().trim().to_string();
                let _ = save_settings(&this.settings);
            }
        }));
        field_subs.push(cx.subscribe(&hotkey_input, |this, e, ev: &TextInputEvent, cx| {
            if let TextInputEvent::Changed = ev {
                let text = e.read(cx).text().trim().to_string();
                if crate::update_global_hotkey(&text) {
                    this.settings.hotkey = text;
                    let _ = save_settings(&this.settings);
                }
            }
        }));
        Self {
            input,
            _input_sub: sub,
            url_input,
            model_input,
            stt_input,
            hotkey_input,
            key_input,
            _field_subs: field_subs,
            undo_text: None,
            translation: None,
            recording: None,
            wave_bars: [0.08; 10],
            status: None,
            is_streaming: false,
            is_recording_hotkey: false,
            settings: s,
            active_view: ActiveView::Input,
            focus_handle: cx.focus_handle(),
            attachments: Vec::new(),
            context,
            image_seq: 0,
            picking_files: false,
            moving_since: None,
            last_origin: Point::default(),
            still_polls: 0,
        }
    }

    // gpui 0.2.2 exposes no activation observer: poll, closing on the active -> inactive edge
    fn watch_click_outside(window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let mut was_active = false;
            loop {
                cx.background_executor().timer(Duration::from_millis(120)).await;
                let Ok(()) = this.update_in(cx, |this, window, _| {
                    let active = window.is_window_active();
                    if let Some(since) = this.moving_since {
                        if active && since.elapsed() > Duration::from_millis(300) || since.elapsed() > Duration::from_secs(30) {
                            this.moving_since = None;
                        }
                        was_active = true;
                        return;
                    }
                    if was_active && !active && this.settings.hide_on_blur && !this.picking_files {
                        window.remove_window();
                    }
                    was_active = active;
                    this.remember_position(window);
                }) else {
                    break;
                };
            }
        })
        .detach();
    }

    // saved once the window has sat still for 2 polls, so dragging doesn't write on every tick
    fn remember_position(&mut self, window: &Window) {
        let origin = window.bounds().origin;
        if origin != self.last_origin {
            self.last_origin = origin;
            self.still_polls = 0;
            return;
        }
        self.still_polls = self.still_polls.saturating_add(1);
        let (x, y) = (f32::from(origin.x), f32::from(origin.y));
        let moved = self.settings.popup_x != Some(x) || self.settings.popup_y != Some(y);
        if self.still_polls == 2 && moved && !self.settings.follow_cursor {
            self.settings.popup_x = Some(x);
            self.settings.popup_y = Some(y);
            let _ = save_settings(&self.settings);
        }
    }

    fn add_attachment(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let image_index = if is_image_path(&path) {
            self.image_seq += 1;
            Some(self.image_seq)
        } else {
            None
        };
        self.attachments.push(Attachment { name, path: path.to_string_lossy().into_owned(), image_index });
        if let Some(n) = image_index {
            self.input.update(cx, |i, cx| i.insert_at_cursor(&format!("[#Image{n}]"), window, cx));
        }
        cx.notify();
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        let removed = self.attachments.remove(index);
        if let Some(n) = removed.image_index {
            let text = self.current_text(cx).replace(&format!("[#Image{n}]"), "");
            self.input.update(cx, |i, cx| i.set_text_silent(&text, cx));
        }
        cx.notify();
    }

    fn paste_clipboard_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = arboard::Clipboard::new()
            .map_err(|e| e.to_string())
            .and_then(|mut cb| cb.get_image().map_err(|e| e.to_string()))
            .and_then(|img| {
                let buf = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
                    .ok_or("bad clipboard image")?;
                let path = std::env::temp_dir().join(format!("jink-{}-{}.png", std::process::id(), self.image_seq + 1));
                buf.save(&path).map_err(|e| e.to_string())?;
                Ok(path)
            });
        match saved {
            Ok(path) => self.add_attachment(path, window, cx),
            Err(_) => {
                self.status = Some(tr(&self.settings.ui_lang, "no_clip").into());
                cx.notify();
            }
        }
    }

    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picking_files = true;
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let paths = picked.await.ok().and_then(|r| r.ok()).flatten().unwrap_or_default();
            let _ = this.update_in(cx, |this, window, cx| {
                this.picking_files = false;
                for path in paths {
                    this.add_attachment(path, window, cx);
                }
                let input_focus = this.input.read(cx).focus_handle(cx);
                window.focus(&input_focus);
            });
        })
        .detach();
    }

    fn image_paths(&self) -> Vec<String> {
        self.attachments.iter().filter(|a| a.image_index.is_some()).map(|a| a.path.clone()).collect()
    }

    pub fn set_hotkey(&mut self, new_hotkey: &str, cx: &mut Context<Self>) {
        self.settings.hotkey = new_hotkey.to_string();
        self.is_recording_hotkey = false;
        self.hotkey_input.update(cx, |i, cx| i.set_text_silent(new_hotkey, cx));
        let _ = save_settings(&self.settings);
        let registered = crate::update_global_hotkey(new_hotkey);
        if registered {
            self.status = Some(format!("Hotkey set to {}", new_hotkey));
        } else {
            self.status = Some(format!("Failed to register hotkey {}", new_hotkey));
        }
        cx.notify();
    }

    fn current_text(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }

    pub fn trigger_inject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut to_inject = self.translation.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| self.current_text(cx));
        let files: Vec<&str> = self.attachments.iter().filter(|a| a.image_index.is_none()).map(|a| a.path.as_str()).collect();
        if !files.is_empty() {
            if !to_inject.trim().is_empty() {
                to_inject.push('\n');
            }
            to_inject.push_str(&files.join("\n"));
        }
        let images: Vec<(u32, String)> = self
            .attachments
            .iter()
            .filter_map(|a| a.image_index.map(|n| (n, a.path.clone())))
            .collect();
        if to_inject.trim().is_empty() && images.is_empty() {
            return;
        }
        window.remove_window();

        std::thread::spawn(move || {
            let _ = inject::inject_text(&to_inject, &images);
        });
    }

    /// What an AI action works on: the input, or the selected-text context when the input
    /// is empty. Returns (text, background context) — the context is only background when
    /// the input has its own text.
    fn action_source(&self, cx: &App) -> Option<(String, Option<String>)> {
        let text = self.current_text(cx);
        if !text.trim().is_empty() {
            return Some((text, self.context.clone()));
        }
        self.context.clone().filter(|c| !c.trim().is_empty()).map(|c| (c, None))
    }

    pub fn trigger_ai_enhance(&mut self, cx: &mut Context<Self>) {
        let Some((text, context)) = self.action_source(cx) else { return };
        if self.is_streaming {
            return;
        }
        let prompt_messages = ai::enhance_prompt(&text, &self.settings.lang_a, &self.settings.lang_b, context.as_deref());
        self.start_ai_stream(prompt_messages, Vec::new(), false, cx);
    }

    pub fn trigger_ai_refine(&mut self, cx: &mut Context<Self>) {
        let Some((text, context)) = self.action_source(cx) else { return };
        if self.is_streaming {
            return;
        }
        let prompt_messages = ai::refine_prompt(&text, &self.settings.lang_a, &self.settings.lang_b, context.as_deref());
        self.start_ai_stream(prompt_messages, Vec::new(), false, cx);
    }

    pub fn trigger_ai_translate(&mut self, reverse: bool, cx: &mut Context<Self>) {
        let Some((text, _)) = self.action_source(cx) else { return };
        if self.is_streaming {
            return;
        }
        let _ = reverse; // direction is auto-detected between the two main languages
        let prompt_messages = ai::translate_prompt(&text, &self.settings.lang_a, &self.settings.lang_b);
        self.start_ai_stream(prompt_messages, Vec::new(), true, cx);
    }

    /// Mic button / Cmd+M: toggle continuous voice-to-text with auto-stop on silence.
    pub fn toggle_mic(&mut self, cx: &mut Context<Self>) {
        let lang = self.settings.ui_lang.clone();
        if let Some(rec) = self.recording.take() {
            rec.finish();
            self.status = None;
            self.wave_bars = [0.08; 10];
            cx.notify();
            return;
        }

        if crate::mic::perm::check() == crate::mic::Access::Denied {
            crate::mic::perm::open_settings();
            self.status = Some(tr(&lang, "mic_blocked").into());
            cx.notify();
            return;
        }

        let is_local = self.settings.ai_base_url.contains("localhost") || self.settings.ai_base_url.contains("127.0.0.1");
        let native = crate::stt::native_available();
        if self.settings.ai_key.trim().is_empty() && !is_local && !native {
            self.status = Some(tr(&lang, "no_key").into());
            self.active_view = ActiveView::Settings;
            cx.notify();
            return;
        }

        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<crate::mic::MicChunk>();
        let rec = match crate::mic::start(false, chunk_tx) {
            Ok(r) => r,
            Err(e) => {
                self.status = Some(format!("Mic: {e}"));
                cx.notify();
                return;
            }
        };

        self.recording = Some(rec);
        self.status = Some(tr(&lang, "mic_rec").into());
        self.wave_bars = [0.12; 10];
        cx.notify();

        // waveform animation, ~30fps
        cx.spawn(async move |entity, cx| {
            let mut tick = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                tick = tick.wrapping_add(1);
                let Ok(should_continue) = entity.update(cx, |this, cx| {
                    if let Some(r) = &this.recording {
                        if !r.is_alive() {
                            this.recording = None;
                            this.wave_bars = [0.08; 10];
                            cx.notify();
                            return false;
                        }
                        let lvl = r.level();
                        for i in 0..10 {
                            let phase = (tick as f32 * 0.35 + i as f32 * 0.7).sin();
                            let target = (lvl * (0.55 + 0.45 * phase)).clamp(0.06, 1.0);
                            this.wave_bars[i] = this.wave_bars[i] * 0.5 + target * 0.5;
                        }
                        cx.notify();
                        true
                    } else {
                        false
                    }
                }) else {
                    break;
                };
                if !should_continue {
                    break;
                }
            }
        })
        .detach();

        // utterances -> text, until auto-stop
        let (base, key, model) = (
            self.settings.ai_base_url.clone(),
            self.settings.ai_key.clone(),
            self.settings.stt_model.clone(),
        );
        let langs = vec![self.settings.lang_a.clone(), self.settings.lang_b.clone()];

        cx.spawn(async move |entity, cx| {
            while let Some(chunk) = chunk_rx.recv().await {
                match chunk {
                    crate::mic::MicChunk::Partial { .. } => {}
                    crate::mic::MicChunk::AutoStop { heard_speech } => {
                        let _ = entity.update(cx, |this, cx| {
                            this.recording = None;
                            this.wave_bars = [0.08; 10];
                            if !heard_speech && this.input.read(cx).is_blank() {
                                let l = this.settings.ui_lang.clone();
                                this.status = Some(tr(&l, "mic_silent").into());
                            } else {
                                this.status = None;
                            }
                            cx.notify();
                        });
                        break;
                    }
                    crate::mic::MicChunk::Utterance { samples, rate, .. } => {
                        let wav = crate::mic::to_wav(&samples, rate);
                        let is_nat = native;
                        let l_clone = langs.clone();
                        let b_clone = base.clone();
                        let k_clone = key.clone();
                        let m_clone = model.clone();

                        // Must run on our Tokio runtime: this task lives on GPUI's executor,
                        // where a bare `tokio::task::spawn` panics and aborts the app.
                        let res = crate::runtime().spawn(async move {
                            if is_nat {
                                tokio::task::spawn_blocking(move || crate::stt::transcribe_native(&wav, &l_clone))
                                    .await
                                    .map_err(|e| e.to_string())
                                    .and_then(|r| r)
                            } else {
                                ai::transcribe(&b_clone, &k_clone, &m_clone, wav).await.map_err(|e| e.to_string())
                            }
                        })
                        .await;

                        let text = match res {
                            Ok(Ok(t)) => t.trim().to_string(),
                            _ => String::new(),
                        };

                        if !text.is_empty() {
                            let prefix = entity
                                .update(cx, |this, cx| {
                                    this.status = None;
                                    let cur = this.current_text(cx);
                                    let lead = if cur.is_empty() || cur.ends_with(char::is_whitespace) { "" } else { " " };
                                    format!("{cur}{lead}")
                                })
                                .unwrap_or_default();
                            let full: Vec<char> = text.chars().collect();
                            let step = (full.len() / 25).max(2);
                            let mut n = 0;
                            while n < full.len() {
                                n = (n + step).min(full.len());
                                let piece: String = full[..n].iter().collect();
                                let current_full = format!("{prefix}{piece}");
                                let _ = entity.update(cx, |this, cx| {
                                    this.input.update(cx, |i, cx| i.set_text_silent(&current_full, cx));
                                    cx.notify();
                                });
                                cx.background_executor().timer(Duration::from_millis(10)).await;
                            }
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Read the translation/explanation card aloud, else the input, else the selected context.
    pub fn trigger_speak(&mut self, cx: &mut Context<Self>) {
        let text = self
            .translation
            .clone()
            .filter(|t| !t.trim().is_empty())
            .or_else(|| self.action_source(cx).map(|(t, _)| t));
        if let Some(text) = text {
            crate::speech::toggle(&text);
        }
    }

    pub fn trigger_ai_explain(&mut self, cx: &mut Context<Self>) {
        let text = self.current_text(cx);
        if (text.trim().is_empty() && self.attachments.is_empty() && self.context.is_none()) || self.is_streaming {
            return;
        }
        let prompt_messages = ai::explain_prompt(&text, &self.settings.lang_a, &self.settings.lang_b, self.context.as_deref());
        let images = self.image_paths();
        // explain shows like translate: original untouched, answer in its own card
        self.start_ai_stream(prompt_messages, images, true, cx);
    }

    pub fn undo_ai(&mut self, cx: &mut Context<Self>) {
        if self.translation.take().is_some() {
            cx.notify();
            return;
        }
        if let Some(prev) = self.undo_text.take() {
            self.input.update(cx, |i, cx| i.set_text_silent(&prev, cx));
            self.status = Some(tr(&self.settings.ui_lang, "undone").into());
            cx.notify();
        }
    }

    pub fn save_current_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = save_settings(&self.settings) {
            self.status = Some(format!("Could not save settings: {e}"));
        } else {
            self.status = Some(tr(&self.settings.ui_lang, "saved").into());
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

    fn start_ai_stream(&mut self, messages: Vec<ai::ChatMessage>, images: Vec<String>, translate: bool, cx: &mut Context<Self>) {
        let is_local = self.settings.ai_base_url.contains("localhost") || self.settings.ai_base_url.contains("127.0.0.1");
        if self.settings.ai_key.trim().is_empty() && !is_local {
            self.status = Some(tr(&self.settings.ui_lang, "no_key").into());
            self.active_view = ActiveView::Settings;
            cx.notify();
            return;
        }

        // the old text stays visible while the model works; the result is typed in afterwards
        if translate {
            self.translation = Some(String::new());
        } else {
            self.undo_text = Some(self.current_text(cx));
            self.translation = None;
        }
        self.is_streaming = true;
        self.status = Some(tr(&self.settings.ui_lang, "writing").into());
        cx.notify();

        let config = AiConfig {
            base_url: self.settings.ai_base_url.clone(),
            model: self.settings.ai_model.clone(),
            api_key: self.settings.ai_key.clone(),
            search_key: self.settings.search_key.clone(),
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        crate::runtime().spawn(async move {
            ai::stream_chat(config, messages, images, tx).await;
        });

        // collect tokens, then typewrite the result into the input or the translate card
        cx.spawn(async move |entity, cx| {
            let mut buf = String::new();
            while let Some(event) = rx.recv().await {
                match event {
                    AiStreamEvent::Token(token) => {
                        buf.push_str(&token);
                    }
                    AiStreamEvent::Done => {
                        let cleaned = ai::strip_think_tags(&buf);
                        let chars: Vec<char> = cleaned.chars().collect();
                        let step = (chars.len() / 40).max(2);
                        let mut n = 0;
                        while n < chars.len() {
                            n = (n + step).min(chars.len());
                            let part: String = chars[..n].iter().collect();
                            let _ = entity.update(cx, |this, cx| {
                                if translate {
                                    this.translation = Some(part);
                                } else {
                                    this.input.update(cx, |i, cx| i.set_text_silent(&part, cx));
                                }
                                cx.notify();
                            });
                            cx.background_executor().timer(Duration::from_millis(12)).await;
                        }
                        let _ = entity.update(cx, |this, cx| {
                            if translate {
                                this.translation = Some(cleaned.clone());
                            } else {
                                this.input.update(cx, |i, cx| i.set_text_silent(&cleaned, cx));
                            }
                            this.is_streaming = false;
                            this.status = None;
                            cx.notify();
                        });
                        break;
                    }
                    AiStreamEvent::Error(err) => {
                        let _ = entity.update(cx, |this, cx| {
                            this.is_streaming = false;
                            if translate {
                                this.translation = None;
                            } else {
                                this.undo_text = None;
                            }
                            this.status = Some(format!("Error: {err}"));
                            cx.notify();
                        });
                        break;
                    }
                }
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
                || self.stt_input.read(cx).is_focused(window)
                || self.hotkey_input.read(cx).is_focused(window)
                || self.key_input.read(cx).is_focused(window);
            if self.is_recording_hotkey {
                if !self.focus_handle.is_focused(window) {
                    window.focus(&self.focus_handle);
                }
            } else if !field_focused && !self.focus_handle.is_focused(window) {
                window.focus(&self.focus_handle);
            }
        } else if self.focus_handle.is_focused(window) {
            window.focus(&input_focus);
        }

        // Window grows for the settings panel, shrinks back for the input.
        let want_w = if is_settings { 720.0 } else { 680.0 };
        let want_h = if is_settings {
            540.0
        } else {
            96.0 + self.input.read(cx).visible_height() + if self.attachments.is_empty() && self.context.is_none() { 0.0 } else { 28.0 }
                + self.translation.as_ref().map_or(0.0, |t| {
                    let lines: usize = t.split('\n').map(|l| l.chars().count() / 58 + 1).sum();
                    8.0 + 22.0 * (lines.min(5) as f32)
                })
        };
        let vp = window.viewport_size();
        if (vp.height - px(want_h)).abs() > px(1.0) || (vp.width - px(want_w)).abs() > px(1.0) {
            window.resize(size(px(want_w), px(want_h)));
        }

        let pill = div()
            .track_focus(&self.focus_handle)
            .key_context("JInkPopup")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let mods = &event.keystroke.modifiers;

                if this.is_recording_hotkey {
                    if key == "escape" {
                        this.is_recording_hotkey = false;
                        this.status = None;
                        cx.notify();
                        return;
                    }

                    let is_modifier_key = matches!(
                        key.to_lowercase().as_str(),
                        "control" | "ctrl" | "alt" | "option" | "shift" | "super" | "meta" | "command" | "cmd"
                    );

                    if !is_modifier_key {
                        let mut parts = Vec::new();
                        if mods.control {
                            parts.push("Control");
                        }
                        if mods.alt {
                            parts.push("Alt");
                        }
                        if mods.shift {
                            parts.push("Shift");
                        }
                        if mods.platform {
                            #[cfg(target_os = "macos")]
                            parts.push("Cmd");
                            #[cfg(not(target_os = "macos"))]
                            parts.push("Super");
                        }

                        let key_name = match key.to_lowercase().as_str() {
                            "space" => "Space".to_string(),
                            "enter" | "return" => "Enter".to_string(),
                            "tab" => "Tab".to_string(),
                            "backspace" => "Backspace".to_string(),
                            other => {
                                if other.len() == 1 {
                                    other.to_uppercase()
                                } else {
                                    let mut c = other.chars();
                                    match c.next() {
                                        None => String::new(),
                                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                                    }
                                }
                            }
                        };
                        parts.push(&key_name);

                        let recorded = parts.join("+");
                        this.set_hotkey(&recorded, cx);
                        return;
                    }
                    return;
                }

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
                        "r" => this.trigger_ai_refine(cx),
                        "e" => this.trigger_ai_enhance(cx),
                        "t" => this.trigger_ai_translate(mods.shift, cx),
                        "enter" => this.trigger_ai_explain(cx),
                        "l" => this.trigger_speak(cx),
                        "m" => this.toggle_mic(cx),
                        "o" => this.pick_files(window, cx),
                        _ => {}
                    }
                }
            }))
            .flex()
            .flex_row()
            .size_full()
            .font_family(FONT)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                for path in paths.paths() {
                    this.add_attachment(path.clone(), window, cx);
                }
            }))
            .bg(Theme::pill_bg())
            .border_1()
            .border_color(Theme::glass_edge())
            .rounded(px(12.0))
            .overflow_hidden()
            // vermilion spine
            .child(div().w(px(3.0)).flex_none().bg(Theme::seal()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .child(if is_settings {
                        div()
                            .id("settings-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(self.render_settings_view(cx))
                            .into_any_element()
                    } else {
                        self.render_input_view(cx).into_any_element()
                    }),
            );

        pill
    }
}

impl JInkPopup {
    fn render_waveform(&self) -> impl IntoElement {
        let mut bars = div().flex().items_center().gap(px(2.0)).h(px(14.0)).px(px(2.0));
        for &h in &self.wave_bars {
            let bar_h = (h * 14.0).clamp(2.5, 14.0);
            bars = bars.child(
                div()
                    .w(px(2.5))
                    .h(px(bar_h))
                    .rounded(px(1.0))
                    .bg(Theme::seal()),
            );
        }
        bars
    }
    fn render_lang_toggle(&mut self, _cx: &mut Context<Self>) -> impl IntoElement {
        let label = format!(
            "{} · {}",
            self.settings.lang_a.to_uppercase(),
            self.settings.lang_b.to_uppercase()
        );
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(div().text_xs().text_color(Theme::paper()).child(label))
    }

    fn render_attachments(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().flex().flex_wrap().gap_1p5();
        if let Some(context) = &self.context {
            let preview: String = context.split_whitespace().collect::<Vec<_>>().join(" ");
            let preview = if preview.chars().count() > 40 { format!("{}…", preview.chars().take(40).collect::<String>()) } else { preview };
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .rounded(px(4.0))
                    .text_xs()
                    .bg(Theme::chip_hover())
                    .border_1()
                    .border_color(Theme::pill_border())
                    .text_color(Theme::paper())
                    .child(format!("◎ context: {preview}"))
                    .child(
                        div()
                            .text_color(Theme::hint())
                            .hover(|st| st.text_color(Theme::seal()).cursor_pointer())
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.context = None;
                                cx.notify();
                            }))
                            .child("×"),
                    ),
            );
        }
        for (i, a) in self.attachments.iter().enumerate() {
            let label = match a.image_index {
                Some(n) => format!("🖼 #{n} {}", a.name),
                None => format!("📎 {}", a.name),
            };
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .rounded(px(4.0))
                    .text_xs()
                    .bg(Theme::chip_hover())
                    .border_1()
                    .border_color(Theme::pill_border())
                    .text_color(Theme::paper())
                    .child(label)
                    .child(
                        div()
                            .text_color(Theme::hint())
                            .hover(|st| st.text_color(Theme::seal()).cursor_pointer())
                            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                                this.remove_attachment(i, cx);
                            }))
                            .child("×"),
                    ),
            );
        }
        row
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
                            .flex_1()
                            .items_center()
                            .gap_2()
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, _| {
                                this.moving_since = Some(Instant::now());
                                window.start_window_move();
                            }))
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
                                div().into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(if self.recording.is_some() {
                                self.render_waveform().into_any_element()
                            } else {
                                div().into_any_element()
                            })
                            .child(self.render_lang_toggle(cx))
                            .child(
                                div()
                                    .p_1()
                                    .rounded(px(4.0))
                                    .bg(if crate::speech::is_speaking() { Theme::seal() } else { gpui::rgba(0x00000000) })
                                    .hover(|st| st.bg(Theme::chip_hover()).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.trigger_speak(cx);
                                    }))
                                    .child(Self::icon(
                                        "volume",
                                        14.0,
                                        if crate::speech::is_speaking() { Theme::ink() } else { Theme::hint() },
                                    )),
                            )
                            .child(
                                div()
                                    .p_1()
                                    .rounded(px(4.0))
                                    .bg(if self.recording.is_some() { Theme::seal() } else { gpui::rgba(0x00000000) })
                                    .hover(|st| st.bg(Theme::chip_hover()).cursor_pointer())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.toggle_mic(cx);
                                    }))
                                    .child(Self::icon(
                                        "mic",
                                        14.0,
                                        if self.recording.is_some() { Theme::ink() } else { Theme::hint() },
                                    )),
                            )
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
            .children(self.translation.clone().map(|t| {
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
                            .text_color(Theme::amber())
                            .child("→"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_h(px(110.0))
                            .overflow_hidden()
                            .text_size(px(14.0))
                            .line_height(px(22.0))
                            .text_color(Theme::paper())
                            .child(if t.is_empty() { "…".to_string() } else { t }),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_xs()
                            .text_color(if crate::speech::is_speaking() { Theme::seal() } else { Theme::hint() })
                            .hover(|st| st.text_color(Theme::paper()).cursor_pointer())
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.trigger_speak(cx);
                            }))
                            .child(Self::icon(
                                "volume",
                                12.0,
                                if crate::speech::is_speaking() { Theme::seal() } else { Theme::hint() },
                            )),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_xs()
                            .text_color(Theme::hint())
                            .hover(|st| st.text_color(Theme::paper()).cursor_pointer())
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.translation = None;
                                cx.notify();
                            }))
                            .child("✕"),
                    )
            }))
            .child(self.render_attachments(cx))
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
                            .child(self.action_button("R", "refine", is_busy, cx.listener(|this, _, _, cx| {
                                this.trigger_ai_refine(cx);
                            })))
                            .child(self.action_button("E", "enhance", is_busy, cx.listener(|this, _, _, cx| {
                                this.trigger_ai_enhance(cx);
                            })))
                            .child(self.action_button("T", "translate", is_busy, cx.listener(|this, _, _, cx| {
                                this.trigger_ai_translate(false, cx);
                            })))
                            .child(self.action_button("L", "speak", false, cx.listener(|this, _, _, cx| {
                                this.trigger_speak(cx);
                            })))
                            .child(self.action_button("O", "attach", false, cx.listener(|this, _, window, cx| {
                                this.pick_files(window, cx);
                            })))
                            .child(self.action_button("↵", "explain", is_busy, cx.listener(|this, _, _, cx| {
                                this.trigger_ai_explain(cx);
                            })))
                            .child(if self.undo_text.is_some() {
                                self.action_button("Z", "undo", false, cx.listener(|this, _, _, cx| {
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
                                    .child(tr(&self.settings.ui_lang, "insert")),
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

        let lang = self.settings.ui_lang.clone();
        let l = lang.as_str();
        let recording = self.is_recording_hotkey;
        let s = self.settings.clone();

        let toggle = |label: &'static str, on: bool| {
            div()
                .px_2()
                .py_1()
                .rounded(px(4.0))
                .text_xs()
                .border_1()
                .border_color(Theme::pill_border())
                .bg(if on { Theme::seal() } else { Theme::chip_hover() })
                .text_color(if on { Theme::ink() } else { Theme::paper() })
                .hover(|st| st.cursor_pointer())
                .child(label)
        };

        let onoff = |on: bool| if on { tr(l, "on") } else { tr(l, "off") };
        let mut toggles = div().flex().flex_col().gap_2();
        toggles = toggles
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(110.0)).flex_none().text_xs().text_color(Theme::hint()).child(tr(l, "follow")))
                    .child(toggle(onoff(s.follow_cursor), s.follow_cursor).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.settings.follow_cursor = !this.settings.follow_cursor;
                            let _ = save_settings(&this.settings);
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div().flex().items_center().gap_2()
                    .child(div().w(px(110.0)).flex_none().text_xs().text_color(Theme::hint()).child(tr(l, "blur")))
                    .child(toggle(onoff(s.hide_on_blur), s.hide_on_blur).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.settings.hide_on_blur = !this.settings.hide_on_blur;
                            let _ = save_settings(&this.settings);
                            cx.notify();
                        }),
                    )),
            );
        if crate::selection::SUPPORTED {
            toggles = toggles
                .child(
                    div().flex().items_center().gap_2()
                        .child(div().w(px(110.0)).flex_none().text_xs().text_color(Theme::hint()).child(tr(l, "context")))
                        .child(toggle(onoff(s.selection_context), s.selection_context).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.settings.selection_context = !this.settings.selection_context;
                                let _ = save_settings(&this.settings);
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    div().flex().items_center().gap_2()
                        .child(div().w(px(110.0)).flex_none().text_xs().text_color(Theme::hint()).child(tr(l, "prefill")))
                        .child(toggle(onoff(s.prefill_selection), s.prefill_selection).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.settings.prefill_selection = !this.settings.prefill_selection;
                                let _ = save_settings(&this.settings);
                                cx.notify();
                            }),
                        )),
                );
        }

        let label = |text: &'static str| {
            div().w(px(110.0)).flex_none().text_xs().text_color(Theme::hint()).child(text)
        };
        let row = || div().flex().flex_wrap().items_center().gap_2();

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
                            .flex_1()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(Theme::seal())
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, _| {
                                this.moving_since = Some(Instant::now());
                                window.start_window_move();
                            }))
                            .child(format!("JINK · {}", tr(l, "settings"))),
                    )
                    .child(chip(tr(l, "back")).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.active_view = ActiveView::Input;
                            cx.notify();
                        }),
                    )),
            )
            .child(div().h(px(1.0)).w_full().bg(Theme::pill_border()))
            .child(Self::field_row("Base URL", self.url_input.clone().into_any_element()))
            .child(Self::field_row("Model", self.model_input.clone().into_any_element()))
            .child(Self::field_row(tr(l, "mic_model"), self.stt_input.clone().into_any_element()))
            .child(Self::field_row("API Key", self.key_input.clone().into_any_element()))
            .child(Self::field_row(tr(l, "hotkey"), self.hotkey_input.clone().into_any_element()))
            .child(
                row()
                    .child(label(""))
                    .children(crate::settings::HOTKEY_PRESETS.iter().map(|(lbl, val)| {
                        let val = *val;
                        chip(lbl).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| this.set_hotkey(val, cx)),
                        )
                    }))
                    .child(
                        toggle(if recording { tr(l, "recording") } else { tr(l, "record") }, recording)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.is_recording_hotkey = !this.is_recording_hotkey;
                                    if this.is_recording_hotkey {
                                        this.status = None;
                                        window.focus(&this.focus_handle);
                                    }
                                    cx.notify();
                                }),
                            ),
                    ),
            )
            .child(
                row()
                    .child(label(tr(l, "preset")))
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
            .child(
                row()
                    .child(label(tr(l, "language")))
                    .child(toggle("English", s.ui_lang != "vi").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.settings.ui_lang = "en".into();
                            let _ = save_settings(&this.settings);
                            cx.notify();
                        }),
                    ))
                    .child(toggle("Tiếng Việt", s.ui_lang == "vi").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.settings.ui_lang = "vi".into();
                            let _ = save_settings(&this.settings);
                            cx.notify();
                        }),
                    )),
            )
            .child(
                row()
                    .child(label(tr(l, "main_langs")))
                    .children(ai::LANGS.iter().map(|c| {
                        let code = *c;
                        toggle(code, s.lang_a == code || s.lang_b == code).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                // exactly two selected: a new pick replaces the oldest one
                                if this.settings.lang_a != code && this.settings.lang_b != code {
                                    this.settings.lang_a = std::mem::replace(&mut this.settings.lang_b, code.to_string());
                                    let _ = save_settings(&this.settings);
                                    cx.notify();
                                }
                            }),
                        )
                    })),
            )
            .child(div().h(px(1.0)).w_full().bg(Theme::pill_border()))
            .child(toggles)
            .children(self.status.clone().map(|st| div().text_xs().text_color(Theme::amber()).child(st)))
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
                    .w(px(110.0))
                    .flex_none()
                    .pt(px(5.0))
                    .text_xs()
                    .text_color(Theme::hint())
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
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
                    .child(format!("{KEY_MOD}{key}")),
            )
            .child(label)
    }
}
