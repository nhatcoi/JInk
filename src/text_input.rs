//! Native multi-line text input for GPUI.
//!
//! Implements `EntityInputHandler` so macOS IME (Telex/VNI, dead keys, emoji
//! palette, dictation) works through marked text, plus selection, word/line
//! navigation, mouse selection, wrapping, scrolling and undo/redo.

use std::ops::Range;
use std::time::{Duration, Instant};

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, ContentMask,
    Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ScrollDelta, ScrollWheelEvent,
    ShapedLine, SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    WrappedLine,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::Theme;

/// Inner wrap width: popup 680 - 2*20 padding - 2*1 border.
pub const WRAP_WIDTH: f32 = 606.0;
/// Max visible text height before the field scrolls (~14 lines; popup grows up to here).
pub const MAX_HEIGHT: f32 = 308.0;
const TYPING_COALESCE: Duration = Duration::from_millis(900);
const UNDO_LIMIT: usize = 200;

actions!(
    jink_input,
    [
        Backspace,
        Delete,
        DeleteWordBack,
        DeleteWordForward,
        DeleteToLineStart,
        DeleteToLineEnd,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocStart,
        DocEnd,
        SelectDocStart,
        SelectDocEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Submit,
        Newline,
        ShowCharacterPalette,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let c = Some("JInkInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("alt-backspace", DeleteWordBack, c),
        KeyBinding::new("ctrl-backspace", DeleteWordBack, c),
        KeyBinding::new("alt-delete", DeleteWordForward, c),
        KeyBinding::new("ctrl-delete", DeleteWordForward, c),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, c),
        KeyBinding::new("ctrl-u", DeleteToLineStart, c),
        KeyBinding::new("ctrl-k", DeleteToLineEnd, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("ctrl-left", WordLeft, c),
        KeyBinding::new("ctrl-right", WordRight, c),
        KeyBinding::new("shift-alt-left", SelectWordLeft, c),
        KeyBinding::new("shift-alt-right", SelectWordRight, c),
        KeyBinding::new("shift-ctrl-left", SelectWordLeft, c),
        KeyBinding::new("shift-ctrl-right", SelectWordRight, c),
        KeyBinding::new("cmd-left", LineStart, c),
        KeyBinding::new("cmd-right", LineEnd, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("shift-cmd-left", SelectLineStart, c),
        KeyBinding::new("shift-cmd-right", SelectLineEnd, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("cmd-up", DocStart, c),
        KeyBinding::new("cmd-down", DocEnd, c),
        KeyBinding::new("ctrl-home", DocStart, c),
        KeyBinding::new("ctrl-end", DocEnd, c),
        KeyBinding::new("shift-cmd-up", SelectDocStart, c),
        KeyBinding::new("shift-cmd-down", SelectDocEnd, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("ctrl-c", Copy, c),
        KeyBinding::new("ctrl-x", Cut, c),
        KeyBinding::new("ctrl-v", Paste, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("cmd-shift-z", Redo, c),
        KeyBinding::new("ctrl-z", Undo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("enter", Submit, c),
        KeyBinding::new("shift-enter", Newline, c),
        KeyBinding::new("alt-enter", Newline, c),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, c),
    ]);
    // emacs line motion is macOS-only: elsewhere Ctrl+A is select-all and Ctrl+E is the refine shortcut
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("ctrl-a", LineStart, c),
        KeyBinding::new("ctrl-e", LineEnd, c),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-a", SelectAll, c),
        KeyBinding::new("ctrl-shift-a", SelectAll, c),
    ]);
}

#[derive(Clone, Debug)]
pub enum TextInputEvent {
    /// User pressed Enter (inject).
    Submit,
    /// Text content changed by the user.
    Changed,
    /// Undo requested but the local history is empty.
    UndoEmpty,
    /// Clipboard held an image and no text on paste.
    PasteImage,
}

#[derive(Clone)]
struct Snapshot {
    content: String,
    selection: Range<usize>,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,

    // Layout cache (filled during request_layout / paint).
    lines: Vec<WrappedLine>,
    line_starts: Vec<usize>,
    line_tops: Vec<Pixels>,
    total_height: Pixels,
    line_height: Pixels,
    placeholder_line: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_y: Pixels,

    goal_x: Option<Pixels>,
    is_selecting: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_typing: Option<Instant>,
    wrap_width: f32,
    single_line: bool,
    masked: bool,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            lines: Vec::new(),
            line_starts: Vec::new(),
            line_tops: Vec::new(),
            total_height: px(0.),
            line_height: px(24.),
            placeholder_line: None,
            last_bounds: None,
            scroll_y: px(0.),
            goal_x: None,
            is_selecting: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_typing: None,
            wrap_width: WRAP_WIDTH,
            single_line: false,
            masked: false,
        }
    }

    /// Single-line settings field (no newlines) with the given wrap width.
    pub fn new_field(
        placeholder: impl Into<SharedString>,
        text: &str,
        wrap_width: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut s = Self::new(placeholder, cx);
        s.wrap_width = wrap_width;
        s.single_line = true;
        s.content = text.to_string();
        let end = s.content.len();
        s.selected_range = end..end;
        s
    }

    // ---------------------------------------------------------------- public

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Show every char as a bullet (API keys). Offsets are mapped to the bullet string.
    pub fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    const BULLET: &'static str = "•";

    /// content byte offset -> display byte offset
    fn disp_off(&self, off: usize) -> usize {
        if !self.masked {
            return off;
        }
        self.content[..off.min(self.content.len())].chars().count() * Self::BULLET.len()
    }

    /// display byte offset -> content byte offset
    fn real_off(&self, d: usize) -> usize {
        if !self.masked {
            return d;
        }
        let n = d / Self::BULLET.len();
        self.content
            .char_indices()
            .nth(n)
            .map(|(i, _)| i)
            .unwrap_or(self.content.len())
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    /// Height of the text area as laid out last frame (clamped to MAX_HEIGHT).
    pub fn visible_height(&self) -> f32 {
        let h: f32 = self.total_height.into();
        h.clamp(f32::from(self.line_height), MAX_HEIGHT)
    }

    pub fn is_blank(&self) -> bool {
        self.content.trim().is_empty()
    }

    /// Replace the whole text without touching undo history (used by AI streaming).
    pub fn set_text_silent(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_string();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_typing = None;
        self.goal_x = None;
        cx.notify();
    }

    #[allow(dead_code)]
    pub fn append_silent(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content.push_str(text);
        let end = self.content.len();
        self.selected_range = end..end;
        cx.notify();
    }

    #[allow(dead_code)]
    pub fn insert_at_cursor(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, text, window, cx);
    }

    // ------------------------------------------------------------- selection

    fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.last_typing = None;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.content.len());
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.last_typing = None;
        cx.notify();
    }

    /// Move or extend selection depending on `select`.
    fn go(&mut self, offset: usize, select: bool, cx: &mut Context<Self>) {
        if select {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    // ------------------------------------------------------------ boundaries

    fn prev_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(i, _)| (i < offset).then_some(i))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(i, _)| (i > offset).then_some(i))
            .unwrap_or(self.content.len())
    }

    fn prev_word(&self, offset: usize) -> usize {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut idx = offset;
        let mut it = self.content[..offset].char_indices().rev().peekable();
        // skip whitespace
        while let Some(&(i, c)) = it.peek() {
            if c.is_whitespace() {
                idx = i;
                it.next();
            } else {
                break;
            }
        }
        if let Some(&(_, first)) = it.peek() {
            let class = is_word(first);
            while let Some(&(i, c)) = it.peek() {
                if !c.is_whitespace() && is_word(c) == class {
                    idx = i;
                    it.next();
                } else {
                    break;
                }
            }
        }
        idx
    }

    fn next_word(&self, offset: usize) -> usize {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut idx = offset;
        let mut it = self.content[offset..].char_indices().peekable();
        while let Some(&(i, c)) = it.peek() {
            if c.is_whitespace() {
                idx = offset + i + c.len_utf8();
                it.next();
            } else {
                break;
            }
        }
        if let Some(&(_, first)) = it.peek() {
            let class = is_word(first);
            while let Some(&(i, c)) = it.peek() {
                if !c.is_whitespace() && is_word(c) == class {
                    idx = offset + i + c.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
        }
        idx
    }

    fn line_start(&self, offset: usize) -> usize {
        self.content[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, offset: usize) -> usize {
        self.content[offset..]
            .find('\n')
            .map(|i| offset + i)
            .unwrap_or(self.content.len())
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let offset = offset.min(self.content.len());
        let mut start = offset;
        for (i, c) in self.content[..offset].char_indices().rev() {
            if is_word(c) {
                start = i;
            } else {
                break;
            }
        }
        let mut end = offset;
        for (i, c) in self.content[offset..].char_indices() {
            if is_word(c) {
                end = offset + i + c.len_utf8();
            } else {
                break;
            }
        }
        if start == end {
            // Clicked on punctuation / whitespace: select that grapheme.
            let n = self.next_boundary(offset);
            return offset..n;
        }
        start..end
    }

    // ------------------------------------------------------- layout geometry

    fn line_index_for_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        }
    }

    /// Position of `offset` relative to the content origin (scroll not applied).
    fn point_for_offset(&self, offset: usize) -> Point<Pixels> {
        if self.lines.is_empty() {
            return point(px(0.), px(0.));
        }
        let offset = self.disp_off(offset);
        let li = self.line_index_for_offset(offset);
        let line = &self.lines[li];
        let local = (offset - self.line_starts[li]).min(line.len());
        let p = line
            .position_for_index(local, self.line_height)
            .unwrap_or_default();
        point(p.x, self.line_tops[li] + p.y)
    }

    /// Byte offset for a point relative to the content origin.
    fn offset_for_point(&self, p: Point<Pixels>) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        if p.y < px(0.) {
            return 0;
        }
        let mut li = self.lines.len() - 1;
        for (i, top) in self.line_tops.iter().enumerate() {
            let h = self.lines[i].size(self.line_height).height;
            if p.y < *top + h {
                li = i;
                break;
            }
        }
        let local = point(p.x, p.y - self.line_tops[li]);
        let idx = match self.lines[li].closest_index_for_position(local, self.line_height) {
            Ok(i) | Err(i) => i,
        };
        let d = self.line_starts[li] + idx.min(self.lines[li].len());
        self.real_off(d).min(self.content.len())
    }

    fn vertical_target(&mut self, dir: i32) -> usize {
        let cur = self.point_for_offset(self.cursor());
        let x = *self.goal_x.get_or_insert(cur.x);
        let y = cur.y + self.line_height * dir as f32 + self.line_height * 0.5;
        if dir < 0 && cur.y <= px(0.) {
            return 0;
        }
        if dir > 0 && y >= self.total_height {
            return self.content.len();
        }
        self.offset_for_point(point(x, y))
    }

    // --------------------------------------------------------------- history

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            content: self.content.clone(),
            selection: self.selected_range.clone(),
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn restore(&mut self, snap: Snapshot, cx: &mut Context<Self>) {
        self.content = snap.content;
        self.selected_range = snap.selection;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_typing = None;
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    // --------------------------------------------------------------- editing

    /// Core edit: replace `range` with `text`, put cursor after it.
    fn edit(&mut self, range: Range<usize>, text: &str, typing: bool, cx: &mut Context<Self>) {
        let now = Instant::now();
        let coalesce = typing
            && self.marked_range.is_none()
            && self
                .last_typing
                .map(|t| now.duration_since(t) < TYPING_COALESCE)
                .unwrap_or(false)
            && !text.chars().any(char::is_whitespace);
        if !coalesce && self.marked_range.is_none() {
            self.push_undo();
        }
        self.last_typing = if typing { Some(now) } else { None };

        self.content.replace_range(range.clone(), text);
        let pos = range.start + text.len();
        self.selected_range = pos..pos;
        self.selection_reversed = false;
        self.marked_range = None;
        self.goal_x = None;
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    fn delete_range_or(&mut self, fallback: Range<usize>, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            fallback
        } else {
            self.selected_range.clone()
        };
        if !range.is_empty() {
            self.edit(range, "", false, cx);
        }
    }

    // --------------------------------------------------------------- actions

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        self.delete_range_or(self.prev_boundary(c)..c, cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        self.delete_range_or(c..self.next_boundary(c), cx);
    }
    fn delete_word_back(&mut self, _: &DeleteWordBack, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        self.delete_range_or(self.prev_word(c)..c, cx);
    }
    fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let c = self.cursor();
        self.delete_range_or(c..self.next_word(c), cx);
    }
    fn delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let c = self.cursor();
        let s = self.line_start(c);
        // At line start, remove the preceding newline instead.
        let s = if s == c { self.prev_boundary(c) } else { s };
        self.delete_range_or(s..c, cx);
    }
    fn delete_to_line_end(&mut self, _: &DeleteToLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let c = self.cursor();
        let e = self.line_end(c);
        let e = if e == c { self.next_boundary(c) } else { e };
        self.delete_range_or(c..e, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.selected_range.is_empty() {
            self.move_to(self.prev_boundary(self.cursor()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(-1);
        let keep = self.goal_x;
        self.move_to(t, cx);
        self.goal_x = keep;
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(1);
        let keep = self.goal_x;
        self.move_to(t, cx);
        self.goal_x = keep;
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.select_to(self.prev_boundary(self.cursor()), cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.select_to(self.next_boundary(self.cursor()), cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(-1);
        let keep = self.goal_x;
        self.select_to(t, cx);
        self.goal_x = keep;
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.vertical_target(1);
        let keep = self.goal_x;
        self.select_to(t, cx);
        self.goal_x = keep;
    }
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.prev_word(self.cursor()), false, cx);
    }
    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.next_word(self.cursor()), false, cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.prev_word(self.cursor()), true, cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.next_word(self.cursor()), true, cx);
    }
    fn line_start_a(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.line_start(self.cursor()), false, cx);
    }
    fn line_end_a(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.line_end(self.cursor()), false, cx);
    }
    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.line_start(self.cursor()), true, cx);
    }
    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.line_end(self.cursor()), true, cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(0, false, cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.content.len(), false, cx);
    }
    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(0, true, cx);
    }
    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.go(self.content.len(), true, cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        self.last_typing = None;
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            let r = self.selected_range.clone();
            self.edit(r, "", false, cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
            if self.single_line {
                text = text.replace('\n', " ");
            }
            let r = self.selected_range.clone();
            self.edit(r, &text, false, cx);
        } else {
            cx.emit(TextInputEvent::PasteImage);
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snap) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.restore(snap, cx);
        } else {
            cx.emit(TextInputEvent::UndoEmpty);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snap) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.restore(snap, cx);
        }
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Submit);
    }
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            return;
        }
        let r = self.selected_range.clone();
        self.edit(r, "\n", false, cx);
    }
    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    // ----------------------------------------------------------------- mouse

    fn mouse_offset(&self, pos: Point<Pixels>) -> usize {
        let Some(b) = self.last_bounds else { return 0 };
        self.offset_for_point(point(pos.x - b.left(), pos.y - b.top() + self.scroll_y))
    }

    fn on_mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.goal_x = None;
        let off = self.mouse_offset(e.position);
        match e.click_count {
            2 => {
                let r = self.word_range_at(off);
                self.selected_range = r;
                self.selection_reversed = false;
                self.is_selecting = false;
                cx.notify();
            }
            n if n >= 3 => {
                self.selected_range = self.line_start(off)..self.line_end(off);
                self.selection_reversed = false;
                self.is_selecting = false;
                cx.notify();
            }
            _ => {
                self.is_selecting = true;
                if e.modifiers.shift {
                    self.select_to(off, cx);
                } else {
                    self.move_to(off, cx);
                }
            }
        }
    }
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }
    fn on_mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let off = self.mouse_offset(e.position);
            self.select_to(off, cx);
        }
    }
    fn on_scroll(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dy = match e.delta {
            ScrollDelta::Pixels(p) => p.y,
            ScrollDelta::Lines(l) => self.line_height * l.y,
        };
        let max = (self.total_height - px(MAX_HEIGHT)).max(px(0.));
        self.scroll_y = (self.scroll_y - dy).clamp(px(0.), max);
        cx.notify();
    }

    // ------------------------------------------------------------ utf16 util

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.content.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(r.start)..self.offset_to_utf16(r.end)
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(r.start)..self.offset_from_utf16(r.end)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.edit(range, new_text, true, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        // One undo step per composition: snapshot only when it starts.
        if self.marked_range.is_none() {
            self.push_undo();
        }
        self.last_typing = None;

        self.content.replace_range(range.clone(), new_text);
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..range.start + new_text.len())
        };
        self.selected_range = new_selected_utf16
            .as_ref()
            .map(|r| {
                // utf16 offsets are relative to the marked text.
                let marked = self.content[range.start..range.start + new_text.len()].to_string();
                let to8 = |u: usize| {
                    let mut utf8 = 0;
                    let mut utf16 = 0;
                    for ch in marked.chars() {
                        if utf16 >= u {
                            break;
                        }
                        utf16 += ch.len_utf16();
                        utf8 += ch.len_utf8();
                    }
                    utf8
                };
                range.start + to8(r.start)..range.start + to8(r.end)
            })
            .unwrap_or_else(|| {
                range.start + new_text.len()..range.start + new_text.len()
            });
        self.selection_reversed = false;
        self.goal_x = None;
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let a = self.point_for_offset(range.start);
        let b = self.point_for_offset(range.end);
        let origin = point(bounds.left() + a.x, bounds.top() + a.y - self.scroll_y);
        Some(Bounds::new(
            origin,
            size((b.x - a.x).max(px(1.)), self.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        pt: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let off = self.mouse_offset(pt);
        Some(self.offset_to_utf16(off))
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .key_context("JInkInput")
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_word_forward))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::delete_to_line_end))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::line_start_a))
            .on_action(cx.listener(Self::line_end_a))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .text_size(px(14.))
            .line_height(px(22.))
            .text_color(Theme::paper())
            .child(TextElement { input: cx.entity() })
    }
}

// ------------------------------------------------------------------ element

struct TextElement {
    input: Entity<TextInput>,
}

struct Prepaint {
    selection: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = Pixels;
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let (content, marked) = {
            let i = self.input.read(cx);
            let marked = i.marked_range.as_ref().map(|m| i.disp_off(m.start)..i.disp_off(m.end));
            let shown = if i.masked {
                TextInput::BULLET.repeat(i.content.chars().count())
            } else {
                i.content.clone()
            };
            (shown, marked)
        };

        let base = TextRun {
            len: content.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs: Vec<TextRun> = match marked {
            Some(m) if m.end <= content.len() => vec![
                TextRun { len: m.start, ..base.clone() },
                TextRun {
                    len: m.end - m.start,
                    underline: Some(UnderlineStyle {
                        color: Some(base.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..base.clone()
                },
                TextRun { len: content.len() - m.end, ..base.clone() },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            _ => vec![base.clone()],
        };

        let lines: Vec<WrappedLine> = if content.is_empty() {
            Vec::new()
        } else {
            window
                .text_system()
                .shape_text(
                    SharedString::from(content.clone()),
                    font_size,
                    &runs,
                    Some(px(self.input.read(cx).wrap_width)),
                    None,
                )
                .map(|v| v.into_iter().collect())
                .unwrap_or_default()
        };

        let mut starts = Vec::with_capacity(lines.len());
        let mut tops = Vec::with_capacity(lines.len());
        let mut off = 0usize;
        let mut y = px(0.);
        for l in &lines {
            starts.push(off);
            tops.push(y);
            off += l.len() + 1; // + '\n'
            y += l.size(line_height).height;
        }
        let total = if lines.is_empty() { line_height } else { y };

        let placeholder = {
            let i = self.input.read(cx);
            if content.is_empty() {
                let run = TextRun {
                    len: i.placeholder.len(),
                    font: style.font(),
                    color: Theme::hint().into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                Some(
                    window
                        .text_system()
                        .shape_line(i.placeholder.clone(), font_size, &[run], None),
                )
            } else {
                None
            }
        };

        let visible = total.min(px(MAX_HEIGHT));
        self.input.update(cx, |i, _| {
            i.lines = lines;
            i.line_starts = starts;
            i.line_tops = tops;
            i.total_height = total;
            i.line_height = line_height;
            i.placeholder_line = placeholder;

            // Keep cursor in view.
            let cy = i.point_for_offset(i.cursor()).y;
            if cy < i.scroll_y {
                i.scroll_y = cy;
            } else if cy + line_height > i.scroll_y + visible {
                i.scroll_y = cy + line_height - visible;
            }
            let max = (total - visible).max(px(0.));
            i.scroll_y = i.scroll_y.clamp(px(0.), max);
        });

        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.).into();
        layout_style.size.height = visible.into();
        (window.request_layout(layout_style, [], cx), visible)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let i = self.input.read(cx);
        let lh = i.line_height;
        let origin = point(bounds.left(), bounds.top() - i.scroll_y);
        let sel = i.selected_range.clone();

        let mut selection = Vec::new();
        let mut cursor = None;

        if sel.is_empty() {
            let p = i.point_for_offset(sel.start);
            cursor = Some(fill(
                Bounds::new(point(origin.x + p.x, origin.y + p.y), size(px(2.), lh)),
                Theme::seal(),
            ));
        } else {
            let color = gpui::rgba(0xe2543a40);
            let a = i.point_for_offset(sel.start);
            let b = i.point_for_offset(sel.end);
            let width = px(i.wrap_width);
            let mut y = a.y;
            while y <= b.y + px(0.5) {
                let x0 = if (y - a.y).abs() < px(0.5) { a.x } else { px(0.) };
                let x1 = if (y - b.y).abs() < px(0.5) { b.x } else { width };
                if x1 > x0 {
                    selection.push(fill(
                        Bounds::new(
                            point(origin.x + x0, origin.y + y),
                            size(x1 - x0, lh),
                        ),
                        color,
                    ));
                }
                y += lh;
            }
        }
        Prepaint { selection, cursor }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        let (lines, tops, lh, scroll, placeholder, total) = {
            let i = self.input.read(cx);
            (
                i.lines.clone(),
                i.line_tops.clone(),
                i.line_height,
                i.scroll_y,
                i.placeholder_line.clone(),
                i.total_height,
            )
        };

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for q in prepaint.selection.drain(..) {
                window.paint_quad(q);
            }
            if let Some(p) = placeholder {
                let _ = p.paint(bounds.origin, lh, window, cx);
            }
            for (line, top) in lines.iter().zip(tops.iter()) {
                let origin = point(bounds.left(), bounds.top() + *top - scroll);
                let _ = line.paint(origin, lh, TextAlign::Left, None, window, cx);
            }
            if focus.is_focused(window) {
                if let Some(c) = prepaint.cursor.take() {
                    window.paint_quad(c);
                }
            }
        });

        // Thin scrollbar on the right edge once the text overflows.
        let view = bounds.size.height;
        if total > view + px(1.) {
            let thumb_h = (view * (view / total)).max(px(24.));
            let max_scroll = total - view;
            let thumb_y = bounds.top() + (view - thumb_h) * (scroll / max_scroll).clamp(0., 1.);
            let thumb = Bounds::new(point(bounds.right() + px(6.), thumb_y), size(px(3.), thumb_h));
            window.paint_quad(fill(thumb, gpui::rgba(0xffffff38)).corner_radii(px(1.5)));
        }

        self.input.update(cx, |i, _| i.last_bounds = Some(bounds));
    }
}
