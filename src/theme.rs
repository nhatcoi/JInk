use gpui::{rgb, rgba};

#[allow(dead_code)]
pub struct Theme;

#[allow(dead_code)]
impl Theme {
    // Catppuccin Mocha Palette
    pub const CRUST: u32 = 0x11111b;       // Deep background
    pub const MANTLE: u32 = 0x181825;      // Window surface background
    pub const BASE: u32 = 0x1e1e2e;        // Input background
    pub const SURFACE0: u32 = 0x313244;    // Borders / dividers
    pub const SURFACE1: u32 = 0x45475a;    // Hover states
    pub const SURFACE2: u32 = 0x585b70;    // Active buttons
    
    pub const TEXT: u32 = 0xcdd6f4;        // Main text
    pub const SUBTEXT0: u32 = 0xa6adc8;    // Secondary text
    pub const OVERLAY0: u32 = 0x6c7086;    // Muted / hints
    
    // Accents
    pub const BLUE: u32 = 0x89b4fa;        // Focus rings / Primary accent
    pub const GREEN: u32 = 0xa6e3a1;       // Success
    pub const YELLOW: u32 = 0xf9e2af;      // Warning / Status
    pub const RED: u32 = 0xf38ba8;         // Error / Cancel
    pub const MAUVE: u32 = 0xcba6f7;       // AI Enhance sparkles
    pub const TEAL: u32 = 0x94e2d5;        // Translate icon

    // "Ink & seal": warm ink black, paper-cream text, one vermilion accent.
    pub const INK: u32 = 0x13110e;      // card background
    pub const KEY: u32 = 0x221f1a;      // keycap / hover surface
    pub const RULE: u32 = 0x2f2a24;     // hairlines / border
    pub const PAPER: u32 = 0xede6d6;    // main text
    pub const DIM: u32 = 0x7d7568;      // muted text
    pub const SEAL: u32 = 0xe2543a;     // vermilion accent
    pub const AMBER: u32 = 0xd9a441;    // status

    pub fn pill_bg() -> gpui::Rgba { rgba(0x13110e9e) } // translucent ink over blur
    pub fn glass_edge() -> gpui::Rgba { rgba(0xffffff26) }
    pub fn pill_border() -> gpui::Rgba { rgba(0xffffff1f) }
    pub fn chip_hover() -> gpui::Rgba { rgba(0xffffff14) }
    pub fn send_blue() -> gpui::Rgba { rgb(Self::SEAL) }
    pub fn hint() -> gpui::Rgba { rgb(Self::DIM) }
    pub fn paper() -> gpui::Rgba { rgb(Self::PAPER) }
    pub fn seal() -> gpui::Rgba { rgb(Self::SEAL) }
    pub fn amber() -> gpui::Rgba { rgb(Self::AMBER) }
    pub fn ink() -> gpui::Rgba { rgb(Self::INK) }
    pub fn crust() -> gpui::Rgba { rgb(Self::CRUST) }
    pub fn mantle() -> gpui::Rgba { rgb(Self::MANTLE) }
    pub fn base() -> gpui::Rgba { rgb(Self::BASE) }
    pub fn surface0() -> gpui::Rgba { rgb(Self::SURFACE0) }
    pub fn surface1() -> gpui::Rgba { rgb(Self::SURFACE1) }
    pub fn surface2() -> gpui::Rgba { rgb(Self::SURFACE2) }
    pub fn text() -> gpui::Rgba { rgb(Self::TEXT) }
    pub fn subtext0() -> gpui::Rgba { rgb(Self::SUBTEXT0) }
    pub fn overlay0() -> gpui::Rgba { rgb(Self::OVERLAY0) }
    pub fn blue() -> gpui::Rgba { rgb(Self::BLUE) }
    pub fn green() -> gpui::Rgba { rgb(Self::GREEN) }
    pub fn yellow() -> gpui::Rgba { rgb(Self::YELLOW) }
    pub fn red() -> gpui::Rgba { rgb(Self::RED) }
    pub fn mauve() -> gpui::Rgba { rgb(Self::MAUVE) }
    pub fn teal() -> gpui::Rgba { rgb(Self::TEAL) }
}

