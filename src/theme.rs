use gpui::rgb;

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

