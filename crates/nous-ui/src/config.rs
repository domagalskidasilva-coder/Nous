//! Configuração pura em Rust (zero dependências) para aparência, layout,
//! atalhos e perfil de desempenho do Nous.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Rgba8, Theme};

pub const DEFAULT_SIDEBAR_WIDTH: f64 = 220.0;
pub const COLLAPSED_SIDEBAR_WIDTH: f64 = 52.0;
pub const DEFAULT_CORNER_RADIUS: f64 = 9.0;
pub const DEFAULT_UI_FONT_SIZE: f32 = 13.0;
pub const DEFAULT_UI_SCALE: f64 = 1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    Hidden,
}

impl TabPosition {
    pub const ALL: [TabPosition; 5] = [
        TabPosition::Top,
        TabPosition::Bottom,
        TabPosition::Left,
        TabPosition::Right,
        TabPosition::Hidden,
    ];

    pub fn is_vertical(self) -> bool {
        matches!(self, TabPosition::Left | TabPosition::Right)
    }

    pub fn is_horizontal(self) -> bool {
        matches!(self, TabPosition::Top | TabPosition::Bottom)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TabPosition::Top => "top",
            TabPosition::Bottom => "bottom",
            TabPosition::Left => "left",
            TabPosition::Right => "right",
            TabPosition::Hidden => "hidden",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "top" | "topo" => Some(TabPosition::Top),
            "bottom" | "baixo" => Some(TabPosition::Bottom),
            "left" | "esquerda" => Some(TabPosition::Left),
            "right" | "direita" => Some(TabPosition::Right),
            "hidden" | "zen" | "oculto" => Some(TabPosition::Hidden),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            TabPosition::Top => TabPosition::Left,
            TabPosition::Left => TabPosition::Right,
            TabPosition::Right => TabPosition::Bottom,
            TabPosition::Bottom => TabPosition::Hidden,
            TabPosition::Hidden => TabPosition::Top,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolbarPosition {
    #[default]
    Top,
    Bottom,
    Unified,
    AutoHide,
}

impl ToolbarPosition {
    pub const ALL: [ToolbarPosition; 4] = [
        ToolbarPosition::Top,
        ToolbarPosition::Bottom,
        ToolbarPosition::Unified,
        ToolbarPosition::AutoHide,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ToolbarPosition::Top => "top",
            ToolbarPosition::Bottom => "bottom",
            ToolbarPosition::Unified => "unified",
            ToolbarPosition::AutoHide => "auto-hide",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match normalize_key(s).as_str() {
            "top" | "topo" => Some(ToolbarPosition::Top),
            "bottom" | "baixo" => Some(ToolbarPosition::Bottom),
            "unified" | "unificada" => Some(ToolbarPosition::Unified),
            "autohide" => Some(ToolbarPosition::AutoHide),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Comfortable,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Compact, Density::Normal, Density::Comfortable];

    pub fn as_str(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Normal => "normal",
            Density::Comfortable => "comfortable",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match normalize_key(s).as_str() {
            "compact" | "compacto" | "compacta" => Some(Density::Compact),
            "normal" | "padrao" => Some(Density::Normal),
            "comfortable" | "confortavel" => Some(Density::Comfortable),
            _ => None,
        }
    }

    pub const fn tabstrip_height(self) -> f64 {
        match self {
            Density::Compact => 34.0,
            Density::Normal => 40.0,
            Density::Comfortable => 48.0,
        }
    }

    pub const fn toolbar_height(self) -> f64 {
        match self {
            Density::Compact => 36.0,
            Density::Normal => 44.0,
            Density::Comfortable => 52.0,
        }
    }

    pub const fn sidebar_row_height(self) -> f64 {
        match self {
            Density::Compact => 28.0,
            Density::Normal => 34.0,
            Density::Comfortable => 40.0,
        }
    }

    pub const fn button_size(self) -> f64 {
        match self {
            Density::Compact => 28.0,
            Density::Normal => 32.0,
            Density::Comfortable => 36.0,
        }
    }

    pub const fn padding(self) -> f64 {
        match self {
            Density::Compact => 6.0,
            Density::Normal => 8.0,
            Density::Comfortable => 10.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePreset {
    #[default]
    NousDark,
    NousLight,
    MidnightOled,
    Nord,
    CyberAmber,
    TerminalMono,
    Custom,
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 7] = [
        ThemePreset::NousDark,
        ThemePreset::NousLight,
        ThemePreset::MidnightOled,
        ThemePreset::Nord,
        ThemePreset::CyberAmber,
        ThemePreset::TerminalMono,
        ThemePreset::Custom,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ThemePreset::NousDark => "nous-dark",
            ThemePreset::NousLight => "nous-light",
            ThemePreset::MidnightOled => "midnight-oled",
            ThemePreset::Nord => "nord",
            ThemePreset::CyberAmber => "cyber-amber",
            ThemePreset::TerminalMono => "terminal-mono",
            ThemePreset::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match normalize_key(s).as_str() {
            "nousdark" | "dark" | "escuro" => Some(ThemePreset::NousDark),
            "nouslight" | "light" | "claro" => Some(ThemePreset::NousLight),
            "midnightoled" | "oled" => Some(ThemePreset::MidnightOled),
            "nord" => Some(ThemePreset::Nord),
            "cyberamber" | "amber" => Some(ThemePreset::CyberAmber),
            "terminalmono" | "terminal" | "mono" => Some(ThemePreset::TerminalMono),
            "custom" | "personalizado" => Some(ThemePreset::Custom),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeOverrides {
    pub dark: Option<bool>,
    pub frame: Option<Rgba8>,
    pub surface: Option<Rgba8>,
    pub hover: Option<Rgba8>,
    pub field: Option<Rgba8>,
    pub text: Option<Rgba8>,
    pub text_dim: Option<Rgba8>,
    pub separator: Option<Rgba8>,
    pub accent: Option<Rgba8>,
    pub accent2: Option<Rgba8>,
    pub danger: Option<Rgba8>,
}

impl ThemeOverrides {
    pub fn apply(&self, mut base: Theme) -> Theme {
        if let Some(d) = self.dark {
            base.dark = d;
        }
        if let Some(c) = self.frame {
            base.frame = c;
        }
        if let Some(c) = self.surface {
            base.surface = c;
        }
        if let Some(c) = self.hover {
            base.hover = c;
        }
        if let Some(c) = self.field {
            base.field = c;
        }
        if let Some(c) = self.text {
            base.text = c;
        }
        if let Some(c) = self.text_dim {
            base.text_dim = c;
        }
        if let Some(c) = self.separator {
            base.separator = c;
        }
        if let Some(c) = self.accent {
            base.accent = c;
        }
        if let Some(c) = self.accent2 {
            base.accent2 = c;
        }
        if let Some(c) = self.danger {
            base.danger = c;
        }
        base
    }

    pub fn is_empty(&self) -> bool {
        self.dark.is_none()
            && self.frame.is_none()
            && self.surface.is_none()
            && self.hover.is_none()
            && self.field.is_none()
            && self.text.is_none()
            && self.text_dim.is_none()
            && self.separator.is_none()
            && self.accent.is_none()
            && self.accent2.is_none()
            && self.danger.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarModules {
    pub back: bool,
    pub forward: bool,
    pub reload: bool,
    pub home: bool,
    pub omnibox: bool,
    pub ram_badge: bool,
    pub perf_badge: bool,
    pub sidebar_toggle: bool,
    pub menu: bool,
}

impl Default for ToolbarModules {
    fn default() -> Self {
        Self {
            back: true,
            forward: true,
            reload: true,
            home: true,
            omnibox: true,
            ram_badge: true,
            perf_badge: true,
            sidebar_toggle: true,
            menu: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PerformanceProfile {
    UltraLight,
    #[default]
    Balanced,
    MaxSpeed,
}

impl PerformanceProfile {
    pub const ALL: [PerformanceProfile; 3] = [
        PerformanceProfile::UltraLight,
        PerformanceProfile::Balanced,
        PerformanceProfile::MaxSpeed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PerformanceProfile::UltraLight => "ultra-light",
            PerformanceProfile::Balanced => "balanced",
            PerformanceProfile::MaxSpeed => "max-speed",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            PerformanceProfile::UltraLight => "ECO",
            PerformanceProfile::Balanced => "BAL",
            PerformanceProfile::MaxSpeed => "MAX",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            PerformanceProfile::UltraLight => "UltraLight",
            PerformanceProfile::Balanced => "Balanced",
            PerformanceProfile::MaxSpeed => "MaxSpeed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match normalize_key(s).as_str() {
            "ultralight" | "eco" | "lightweight" => Some(PerformanceProfile::UltraLight),
            "balanced" | "balanceado" | "normal" => Some(PerformanceProfile::Balanced),
            "maxspeed" | "max" | "fast" | "performance" => Some(PerformanceProfile::MaxSpeed),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            PerformanceProfile::UltraLight => PerformanceProfile::Balanced,
            PerformanceProfile::Balanced => PerformanceProfile::MaxSpeed,
            PerformanceProfile::MaxSpeed => PerformanceProfile::UltraLight,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShortcutAction {
    NewTab,
    CloseTab,
    FocusOmnibox,
    Reload,
    Back,
    Forward,
    NextTab,
    PrevTab,
    ToggleSidebar,
    ToggleZenMode,
    ToggleTheme,
    OpenSettings,
    OpenMemory,
    CyclePerfProfile,
}

impl ShortcutAction {
    pub const ALL: [ShortcutAction; 14] = [
        ShortcutAction::NewTab,
        ShortcutAction::CloseTab,
        ShortcutAction::FocusOmnibox,
        ShortcutAction::Reload,
        ShortcutAction::Back,
        ShortcutAction::Forward,
        ShortcutAction::NextTab,
        ShortcutAction::PrevTab,
        ShortcutAction::ToggleSidebar,
        ShortcutAction::ToggleZenMode,
        ShortcutAction::ToggleTheme,
        ShortcutAction::OpenSettings,
        ShortcutAction::OpenMemory,
        ShortcutAction::CyclePerfProfile,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ShortcutAction::NewTab => "new_tab",
            ShortcutAction::CloseTab => "close_tab",
            ShortcutAction::FocusOmnibox => "focus_omnibox",
            ShortcutAction::Reload => "reload",
            ShortcutAction::Back => "back",
            ShortcutAction::Forward => "forward",
            ShortcutAction::NextTab => "next_tab",
            ShortcutAction::PrevTab => "prev_tab",
            ShortcutAction::ToggleSidebar => "toggle_sidebar",
            ShortcutAction::ToggleZenMode => "toggle_zen_mode",
            ShortcutAction::ToggleTheme => "toggle_theme",
            ShortcutAction::OpenSettings => "open_settings",
            ShortcutAction::OpenMemory => "open_memory",
            ShortcutAction::CyclePerfProfile => "cycle_perf_profile",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match normalize_key(s).as_str() {
            "newtab" => Some(ShortcutAction::NewTab),
            "closetab" => Some(ShortcutAction::CloseTab),
            "focusomnibox" | "omnibox" => Some(ShortcutAction::FocusOmnibox),
            "reload" => Some(ShortcutAction::Reload),
            "back" => Some(ShortcutAction::Back),
            "forward" => Some(ShortcutAction::Forward),
            "nexttab" => Some(ShortcutAction::NextTab),
            "prevtab" | "previoustab" => Some(ShortcutAction::PrevTab),
            "togglesidebar" => Some(ShortcutAction::ToggleSidebar),
            "togglezenmode" | "zenmode" | "togglezen" => Some(ShortcutAction::ToggleZenMode),
            "toggletheme" => Some(ShortcutAction::ToggleTheme),
            "opensettings" | "settings" => Some(ShortcutAction::OpenSettings),
            "openmemory" | "memory" => Some(ShortcutAction::OpenMemory),
            "cycleperfprofile" | "perfprofile" => Some(ShortcutAction::CyclePerfProfile),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

impl Shortcut {
    pub fn new(ctrl: bool, shift: bool, alt: bool, key: &str) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            key: canonical_key(key),
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut key: Option<String> = None;

        for part in split_shortcut_parts(trimmed) {
            let token = part.trim();
            if token.is_empty() {
                return None;
            }
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "super" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(canonical_key(token));
                }
            }
        }

        let key = key?;
        if key.is_empty() {
            return None;
        }
        Some(Self {
            ctrl,
            shift,
            alt,
            key,
        })
    }

    pub fn matches(&self, ctrl: bool, shift: bool, alt: bool, key: &str) -> bool {
        self.ctrl == ctrl && self.shift == shift && self.alt == alt && self.key == canonical_key(key)
    }

    pub fn format_string(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(&self.key);
        out
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format_string())
    }
}

#[derive(Clone, Debug, Eq)]
pub struct KeyBindings {
    entries: Vec<(Shortcut, ShortcutAction)>,
}

impl PartialEq for KeyBindings {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .all(|pair| other.entries.contains(pair))
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        let defaults = [
            ("Ctrl+T", ShortcutAction::NewTab),
            ("Ctrl+W", ShortcutAction::CloseTab),
            ("Ctrl+L", ShortcutAction::FocusOmnibox),
            ("F6", ShortcutAction::FocusOmnibox),
            ("Ctrl+R", ShortcutAction::Reload),
            ("F5", ShortcutAction::Reload),
            ("Alt+Left", ShortcutAction::Back),
            ("Alt+Right", ShortcutAction::Forward),
            ("Ctrl+Tab", ShortcutAction::NextTab),
            ("Ctrl+Shift+Tab", ShortcutAction::PrevTab),
            ("Ctrl+B", ShortcutAction::ToggleSidebar),
            ("F11", ShortcutAction::ToggleZenMode),
            ("Ctrl+Shift+T", ShortcutAction::ToggleTheme),
            ("Ctrl+,", ShortcutAction::OpenSettings),
            ("Ctrl+Shift+M", ShortcutAction::OpenMemory),
            ("Ctrl+Shift+P", ShortcutAction::CyclePerfProfile),
        ];
        let entries = defaults
            .into_iter()
            .filter_map(|(s, a)| Shortcut::parse(s).map(|sc| (sc, a)))
            .collect();
        Self { entries }
    }
}

impl KeyBindings {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn lookup(&self, ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<ShortcutAction> {
        let target = Shortcut::new(ctrl, shift, alt, key);
        self.match_shortcut(&target)
    }

    pub fn match_shortcut(&self, shortcut: &Shortcut) -> Option<ShortcutAction> {
        self.entries
            .iter()
            .rfind(|(s, _)| s == shortcut)
            .map(|(_, a)| *a)
    }

    pub fn bind(&mut self, shortcut: Shortcut, action: ShortcutAction) {
        if let Some((_, existing)) = self.entries.iter_mut().find(|(s, _)| *s == shortcut) {
            *existing = action;
        } else {
            self.entries.push((shortcut, action));
        }
    }

    pub fn set_for_action(&mut self, action: ShortcutAction, shortcuts: Vec<Shortcut>) {
        self.entries.retain(|(_, a)| *a != action);
        for s in shortcuts {
            self.bind(s, action);
        }
    }

    pub fn shortcuts_for(&self, action: ShortcutAction) -> Vec<&Shortcut> {
        self.entries
            .iter()
            .filter(|(_, a)| *a == action)
            .map(|(s, _)| s)
            .collect()
    }

    pub fn primary_shortcut(&self, action: ShortcutAction) -> Option<&Shortcut> {
        self.entries
            .iter()
            .find(|(_, a)| *a == action)
            .map(|(s, _)| s)
    }

    pub fn entries(&self) -> &[(Shortcut, ShortcutAction)] {
        &self.entries
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NousConfig {
    pub tab_position: TabPosition,
    pub toolbar_position: ToolbarPosition,
    pub sidebar_width: f64,
    pub sidebar_collapsed: bool,
    pub density: Density,
    pub corner_radius: f64,
    pub theme_preset: ThemePreset,
    pub theme_overrides: ThemeOverrides,
    pub ui_font: String,
    pub ui_font_size: f32,
    pub ui_scale: f64,
    pub toolbar_modules: ToolbarModules,
    pub keybindings: KeyBindings,
    pub perf_profile: PerformanceProfile,
    pub tab_sleep_after_secs: u64,
    pub tab_discard_after_secs: u64,
    pub animations_enabled: bool,
    pub block_trackers: bool,
    pub js_enabled: bool,
    pub images_enabled: bool,
    pub max_image_decode_px: u32,
    pub cache_budget_mb: u32,
    pub home_url: String,
    pub search_engine: String,
    pub custom_search_url: Option<String>,
}

impl Default for NousConfig {
    fn default() -> Self {
        let mut cfg = Self {
            tab_position: TabPosition::Top,
            toolbar_position: ToolbarPosition::Top,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            sidebar_collapsed: false,
            density: Density::Normal,
            corner_radius: DEFAULT_CORNER_RADIUS,
            theme_preset: ThemePreset::NousDark,
            theme_overrides: ThemeOverrides::default(),
            ui_font: String::new(),
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            ui_scale: DEFAULT_UI_SCALE,
            toolbar_modules: ToolbarModules::default(),
            keybindings: KeyBindings::default(),
            perf_profile: PerformanceProfile::Balanced,
            tab_sleep_after_secs: 120,
            tab_discard_after_secs: 600,
            animations_enabled: true,
            block_trackers: true,
            js_enabled: true,
            images_enabled: true,
            max_image_decode_px: 1024,
            cache_budget_mb: 8,
            home_url: "nous://nova-aba".to_owned(),
            search_engine: "DuckDuckGo".to_owned(),
            custom_search_url: None,
        };
        cfg.apply_perf_profile(PerformanceProfile::Balanced);
        cfg
    }
}

impl NousConfig {
    pub fn resolved_theme(&self) -> Theme {
        let base = Theme::from_preset(self.theme_preset);
        self.theme_overrides.apply(base)
    }

    pub fn effective_sidebar_width(&self) -> f64 {
        if self.sidebar_collapsed {
            COLLAPSED_SIDEBAR_WIDTH
        } else {
            self.sidebar_width.clamp(120.0, 480.0)
        }
    }

    pub fn effective_corner_radius(&self) -> f64 {
        self.corner_radius.clamp(0.0, 16.0)
    }

    pub fn effective_font(&self) -> Option<&str> {
        let trimmed = self.ui_font.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    pub fn apply_perf_profile(&mut self, profile: PerformanceProfile) {
        self.perf_profile = profile;
        match profile {
            PerformanceProfile::UltraLight => {
                self.tab_sleep_after_secs = 30;
                self.tab_discard_after_secs = 120;
                self.animations_enabled = false;
                self.block_trackers = true;
                self.js_enabled = false;
                self.images_enabled = false;
                self.max_image_decode_px = 512;
                self.cache_budget_mb = 4;
            }
            PerformanceProfile::Balanced => {
                self.tab_sleep_after_secs = 120;
                self.tab_discard_after_secs = 600;
                self.animations_enabled = true;
                self.block_trackers = true;
                self.js_enabled = true;
                self.images_enabled = true;
                self.max_image_decode_px = 1024;
                self.cache_budget_mb = 8;
            }
            PerformanceProfile::MaxSpeed => {
                self.tab_sleep_after_secs = 600;
                self.tab_discard_after_secs = 3600;
                self.animations_enabled = true;
                self.block_trackers = true;
                self.js_enabled = true;
                self.images_enabled = true;
                self.max_image_decode_px = 2048;
                self.cache_budget_mb = 32;
            }
        }
    }

    pub fn clamp_fields(&mut self) {
        self.sidebar_width = clamp_finite(self.sidebar_width, 120.0, 480.0, DEFAULT_SIDEBAR_WIDTH);
        self.corner_radius = clamp_finite(self.corner_radius, 0.0, 16.0, DEFAULT_CORNER_RADIUS);
        self.ui_font_size =
            clamp_finite(self.ui_font_size as f64, 9.0, 24.0, DEFAULT_UI_FONT_SIZE as f64) as f32;
        self.ui_scale = clamp_finite(self.ui_scale, 0.75, 2.0, DEFAULT_UI_SCALE);
        self.max_image_decode_px = self.max_image_decode_px.clamp(64, 8192);
        self.cache_budget_mb = self.cache_budget_mb.min(1024);
    }

    pub fn default_config_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
                return Some(PathBuf::from(appdata).join("Nous").join("config.toml"));
            }
            if let Some(home) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
                return Some(
                    PathBuf::from(home)
                        .join(".config")
                        .join("nous")
                        .join("config.toml"),
                );
            }
            None
        }
        #[cfg(target_os = "macos")]
        {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| {
                    PathBuf::from(home)
                        .join("Library")
                        .join("Application Support")
                        .join("Nous")
                        .join("config.toml")
                })
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
                return Some(PathBuf::from(xdg).join("nous").join("config.toml"));
            }
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| {
                    PathBuf::from(home)
                        .join(".config")
                        .join("nous")
                        .join("config.toml")
                })
        }
    }

    pub fn load_from_path(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self::parse(&content))
    }

    pub fn load_or_default(path: &Path) -> Self {
        Self::load_from_path(path).unwrap_or_default()
    }

    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.serialize())
    }

    pub fn parse(input: &str) -> Self {
        let mut cfg = Self::default();
        let mut section = String::new();
        let mut cleared_actions: Vec<ShortcutAction> = Vec::new();
        let mut perf_knobs_pending: Vec<(String, String)> = Vec::new();

        for raw_line in input.lines() {
            let line = raw_line.trim();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with(';')
                || line.starts_with("//")
            {
                continue;
            }
            if let Some(sec) = parse_section_header(line) {
                section = sec.to_ascii_lowercase();
                continue;
            }
            let Some((raw_key, val)) = parse_key_value(line) else {
                continue;
            };
            let full_key = qualify_key(&section, raw_key);

            match full_key.as_str() {
                "layout.tab_position" | "tab_position" => {
                    if let Some(v) = val.as_scalar().and_then(TabPosition::from_str) {
                        cfg.tab_position = v;
                    }
                }
                "layout.toolbar_position" | "toolbar_position" => {
                    if let Some(v) = val.as_scalar().and_then(ToolbarPosition::from_str) {
                        cfg.toolbar_position = v;
                    }
                }
                "layout.sidebar_width" | "sidebar_width" => {
                    if let Some(v) = val.as_f64() {
                        cfg.sidebar_width = v;
                    }
                }
                "layout.sidebar_collapsed" | "sidebar_collapsed" => {
                    if let Some(v) = val.as_bool() {
                        cfg.sidebar_collapsed = v;
                    }
                }
                "layout.density" | "density" => {
                    if let Some(v) = val.as_scalar().and_then(Density::from_str) {
                        cfg.density = v;
                    }
                }
                "layout.corner_radius" | "corner_radius" => {
                    if let Some(v) = val.as_f64() {
                        cfg.corner_radius = v;
                    }
                }
                "theme.preset" | "theme.theme_preset" | "theme_preset" => {
                    if let Some(v) = val.as_scalar().and_then(ThemePreset::from_str) {
                        cfg.theme_preset = v;
                    }
                }
                "theme.ui_font" | "ui_font" => {
                    if let Some(v) = val.as_scalar() {
                        cfg.ui_font = v.to_owned();
                    }
                }
                "theme.ui_font_size" | "ui_font_size" => {
                    if let Some(v) = val.as_f64() {
                        cfg.ui_font_size = v as f32;
                    }
                }
                "theme.ui_scale" | "ui_scale" => {
                    if let Some(v) = val.as_f64() {
                        cfg.ui_scale = v;
                    }
                }
                "theme.overrides.dark" | "theme.colors.dark" | "theme.dark" | "theme_dark" => {
                    if let Some(v) = val.as_bool() {
                        cfg.theme_overrides.dark = Some(v);
                    }
                }
                "theme.overrides.frame" | "theme.colors.frame" | "theme_frame" => {
                    cfg.theme_overrides.frame = parse_optional_color(&val);
                }
                "theme.overrides.surface" | "theme.colors.surface" | "theme_surface" => {
                    cfg.theme_overrides.surface = parse_optional_color(&val);
                }
                "theme.overrides.hover" | "theme.colors.hover" | "theme_hover" => {
                    cfg.theme_overrides.hover = parse_optional_color(&val);
                }
                "theme.overrides.field" | "theme.colors.field" | "theme_field" => {
                    cfg.theme_overrides.field = parse_optional_color(&val);
                }
                "theme.overrides.text" | "theme.colors.text" | "theme_text" => {
                    cfg.theme_overrides.text = parse_optional_color(&val);
                }
                "theme.overrides.text_dim" | "theme.colors.text_dim" | "theme_text_dim" => {
                    cfg.theme_overrides.text_dim = parse_optional_color(&val);
                }
                "theme.overrides.separator" | "theme.colors.separator" | "theme_separator" => {
                    cfg.theme_overrides.separator = parse_optional_color(&val);
                }
                "theme.overrides.accent" | "theme.colors.accent" | "theme_accent" => {
                    cfg.theme_overrides.accent = parse_optional_color(&val);
                }
                "theme.overrides.accent2" | "theme.colors.accent2" | "theme_accent2" => {
                    cfg.theme_overrides.accent2 = parse_optional_color(&val);
                }
                "theme.overrides.danger" | "theme.colors.danger" | "theme_danger" => {
                    cfg.theme_overrides.danger = parse_optional_color(&val);
                }
                "toolbar.back" | "toolbar_modules.back" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.back = v;
                    }
                }
                "toolbar.forward" | "toolbar_modules.forward" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.forward = v;
                    }
                }
                "toolbar.reload" | "toolbar_modules.reload" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.reload = v;
                    }
                }
                "toolbar.home" | "toolbar_modules.home" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.home = v;
                    }
                }
                "toolbar.omnibox" | "toolbar_modules.omnibox" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.omnibox = v;
                    }
                }
                "toolbar.ram_badge" | "toolbar_modules.ram_badge" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.ram_badge = v;
                    }
                }
                "toolbar.perf_badge" | "toolbar_modules.perf_badge" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.perf_badge = v;
                    }
                }
                "toolbar.sidebar_toggle" | "toolbar_modules.sidebar_toggle" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.sidebar_toggle = v;
                    }
                }
                "toolbar.menu" | "toolbar_modules.menu" => {
                    if let Some(v) = val.as_bool() {
                        cfg.toolbar_modules.menu = v;
                    }
                }
                "performance.profile"
                | "performance.perf_profile"
                | "perf_profile" => {
                    if let Some(v) = val.as_scalar().and_then(PerformanceProfile::from_str) {
                        cfg.apply_perf_profile(v);
                    }
                }
                "performance.tab_sleep_after_secs" | "tab_sleep_after_secs" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("tab_sleep_after_secs".into(), s.to_owned()));
                    }
                }
                "performance.tab_discard_after_secs" | "tab_discard_after_secs" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("tab_discard_after_secs".into(), s.to_owned()));
                    }
                }
                "performance.animations_enabled" | "animations_enabled" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("animations_enabled".into(), s.to_owned()));
                    }
                }
                "performance.block_trackers" | "block_trackers" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("block_trackers".into(), s.to_owned()));
                    }
                }
                "performance.js_enabled" | "js_enabled" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("js_enabled".into(), s.to_owned()));
                    }
                }
                "performance.images_enabled" | "images_enabled" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("images_enabled".into(), s.to_owned()));
                    }
                }
                "performance.max_image_decode_px" | "max_image_decode_px" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("max_image_decode_px".into(), s.to_owned()));
                    }
                }
                "performance.cache_budget_mb" | "cache_budget_mb" => {
                    if let Some(s) = val.as_scalar() {
                        perf_knobs_pending.push(("cache_budget_mb".into(), s.to_owned()));
                    }
                }
                "navigation.home_url" | "home_url" => {
                    if let Some(v) = val.as_scalar() && !v.trim().is_empty() {
                        cfg.home_url = v.trim().to_owned();
                    }
                }
                "navigation.search_engine" | "search_engine" => {
                    if let Some(v) = val.as_scalar() && !v.trim().is_empty() {
                        cfg.search_engine = v.trim().to_owned();
                    }
                }
                "navigation.custom_search_url" | "custom_search_url" => {
                    if let Some(v) = val.as_scalar() {
                        let t = v.trim();
                        cfg.custom_search_url =
                            (!t.is_empty() && !t.eq_ignore_ascii_case("none"))
                                .then(|| t.to_owned());
                    }
                }
                _ => {
                    let action_key = full_key
                        .strip_prefix("keybindings.")
                        .unwrap_or(raw_key);
                    if (full_key.starts_with("keybindings.") || section == "keybindings")
                        && let Some(action) = ShortcutAction::from_str(action_key)
                    {
                        let parsed: Vec<Shortcut> = val
                            .as_list()
                            .into_iter()
                            .filter_map(|s| Shortcut::parse(s))
                            .collect();
                        if !cleared_actions.contains(&action) {
                            cleared_actions.push(action);
                            cfg.keybindings.set_for_action(action, parsed);
                        } else {
                            for sc in parsed {
                                cfg.keybindings.bind(sc, action);
                            }
                        }
                    }
                }
            }
        }

        // Apply explicit performance knobs after profile selection so granular overrides win
        // regardless of line order in the file.
        for (k, v) in perf_knobs_pending {
            match k.as_str() {
                "tab_sleep_after_secs" => {
                    if let Ok(n) = v.trim().parse::<u64>() {
                        cfg.tab_sleep_after_secs = n;
                    }
                }
                "tab_discard_after_secs" => {
                    if let Ok(n) = v.trim().parse::<u64>() {
                        cfg.tab_discard_after_secs = n;
                    }
                }
                "animations_enabled" => {
                    if let Some(b) = parse_bool(&v) {
                        cfg.animations_enabled = b;
                    }
                }
                "block_trackers" => {
                    if let Some(b) = parse_bool(&v) {
                        cfg.block_trackers = b;
                    }
                }
                "js_enabled" => {
                    if let Some(b) = parse_bool(&v) {
                        cfg.js_enabled = b;
                    }
                }
                "images_enabled" => {
                    if let Some(b) = parse_bool(&v) {
                        cfg.images_enabled = b;
                    }
                }
                "max_image_decode_px" => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        cfg.max_image_decode_px = n;
                    }
                }
                "cache_budget_mb" => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        cfg.cache_budget_mb = n;
                    }
                }
                _ => {}
            }
        }

        cfg.clamp_fields();
        cfg
    }

    pub fn serialize(&self) -> String {
        use std::fmt::Write;
        let mut s = String::with_capacity(1400);
        let _ = writeln!(s, "# Nous Browser Configuration");
        let _ = writeln!(s, "\n[layout]");
        let _ = writeln!(s, "tab_position = \"{}\"", self.tab_position.as_str());
        let _ = writeln!(s, "toolbar_position = \"{}\"", self.toolbar_position.as_str());
        let _ = writeln!(s, "sidebar_width = {:.1}", self.sidebar_width);
        let _ = writeln!(s, "sidebar_collapsed = {}", self.sidebar_collapsed);
        let _ = writeln!(s, "density = \"{}\"", self.density.as_str());
        let _ = writeln!(s, "corner_radius = {:.1}", self.effective_corner_radius());

        let _ = writeln!(s, "\n[theme]");
        let _ = writeln!(s, "preset = \"{}\"", self.theme_preset.as_str());
        let _ = writeln!(s, "ui_font = \"{}\"", escape_string(&self.ui_font));
        let _ = writeln!(s, "ui_font_size = {:.1}", self.ui_font_size);
        let _ = writeln!(s, "ui_scale = {:.2}", self.ui_scale);

        let ov = &self.theme_overrides;
        if !ov.is_empty() {
            let _ = writeln!(s, "\n[theme.colors]");
            if let Some(d) = ov.dark {
                let _ = writeln!(s, "dark = {d}");
            }
            for (name, val) in [
                ("frame", ov.frame),
                ("surface", ov.surface),
                ("hover", ov.hover),
                ("field", ov.field),
                ("text", ov.text),
                ("text_dim", ov.text_dim),
                ("separator", ov.separator),
                ("accent", ov.accent),
                ("accent2", ov.accent2),
                ("danger", ov.danger),
            ] {
                if let Some(c) = val {
                    let _ = writeln!(s, "{name} = \"{}\"", format_hex_color(c));
                }
            }
        }

        let tm = &self.toolbar_modules;
        let _ = writeln!(s, "\n[toolbar]");
        let _ = writeln!(s, "back = {}", tm.back);
        let _ = writeln!(s, "forward = {}", tm.forward);
        let _ = writeln!(s, "reload = {}", tm.reload);
        let _ = writeln!(s, "home = {}", tm.home);
        let _ = writeln!(s, "omnibox = {}", tm.omnibox);
        let _ = writeln!(s, "ram_badge = {}", tm.ram_badge);
        let _ = writeln!(s, "perf_badge = {}", tm.perf_badge);
        let _ = writeln!(s, "sidebar_toggle = {}", tm.sidebar_toggle);
        let _ = writeln!(s, "menu = {}", tm.menu);

        let _ = writeln!(s, "\n[performance]");
        let _ = writeln!(s, "profile = \"{}\"", self.perf_profile.as_str());
        let _ = writeln!(s, "tab_sleep_after_secs = {}", self.tab_sleep_after_secs);
        let _ = writeln!(s, "tab_discard_after_secs = {}", self.tab_discard_after_secs);
        let _ = writeln!(s, "animations_enabled = {}", self.animations_enabled);
        let _ = writeln!(s, "block_trackers = {}", self.block_trackers);
        let _ = writeln!(s, "js_enabled = {}", self.js_enabled);
        let _ = writeln!(s, "images_enabled = {}", self.images_enabled);
        let _ = writeln!(s, "max_image_decode_px = {}", self.max_image_decode_px);
        let _ = writeln!(s, "cache_budget_mb = {}", self.cache_budget_mb);

        let _ = writeln!(s, "\n[navigation]");
        let _ = writeln!(s, "home_url = \"{}\"", escape_string(&self.home_url));
        let _ = writeln!(s, "search_engine = \"{}\"", escape_string(&self.search_engine));
        if let Some(custom) = &self.custom_search_url {
            let _ = writeln!(s, "custom_search_url = \"{}\"", escape_string(custom));
        }

        let _ = writeln!(s, "\n[keybindings]");
        for action in ShortcutAction::ALL {
            let shortcuts = self.keybindings.shortcuts_for(action);
            if shortcuts.is_empty() {
                continue;
            }
            if shortcuts.len() == 1 {
                let _ = writeln!(
                    s,
                    "{} = \"{}\"",
                    action.as_str(),
                    escape_string(&shortcuts[0].format_string())
                );
            } else {
                let items: Vec<String> = shortcuts
                    .iter()
                    .map(|sc| format!("\"{}\"", escape_string(&sc.format_string())))
                    .collect();
                let _ = writeln!(s, "{} = [{}]", action.as_str(), items.join(", "));
            }
        }
        s
    }
}

pub fn parse_hex_color(input: &str) -> Option<Rgba8> {
    let hex = input.trim().strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let nib = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok().map(|v| v * 17);
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => Some([nib(0)?, nib(1)?, nib(2)?, 255]),
        4 => Some([nib(0)?, nib(1)?, nib(2)?, nib(3)?]),
        6 => Some([pair(0)?, pair(2)?, pair(4)?, 255]),
        8 => Some([pair(0)?, pair(2)?, pair(4)?, pair(6)?]),
        _ => None,
    }
}

pub fn format_hex_color(c: Rgba8) -> String {
    if c[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }
}

// ------------------------------------------------------------------ parser helpers

enum ConfigValue {
    Scalar(String),
    List(Vec<String>),
}

impl ConfigValue {
    fn as_scalar(&self) -> Option<&str> {
        match self {
            ConfigValue::Scalar(s) => Some(s.as_str()),
            ConfigValue::List(v) => v.first().map(String::as_str),
        }
    }

    fn as_bool(&self) -> Option<bool> {
        self.as_scalar().and_then(parse_bool)
    }

    fn as_f64(&self) -> Option<f64> {
        self.as_scalar().and_then(|s| s.trim().parse::<f64>().ok())
    }

    fn as_list(&self) -> Vec<&str> {
        match self {
            ConfigValue::Scalar(s) => vec![s.as_str()],
            ConfigValue::List(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "sim" | "on" => Some(true),
        "false" | "0" | "no" | "nao" | "não" | "off" => Some(false),
        _ => None,
    }
}

fn clamp_finite(v: f64, min: f64, max: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v.clamp(min, max)
    } else {
        fallback
    }
}

fn normalize_key(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

fn split_shortcut_parts(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            if i == start && i + 1 == bytes.len() {
                // "+" key itself at the end (e.g. "Ctrl++")
                break;
            }
            if i > start {
                parts.push(&s[start..i]);
                start = i + 1;
            }
        }
        i += 1;
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}

fn canonical_key(raw: &str) -> String {
    let t = raw.trim();
    if t.len() == 1 {
        let ch = t.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            return ch.to_ascii_uppercase().to_string();
        }
        return ch.to_string();
    }
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "left" | "arrowleft" => "Left".into(),
        "right" | "arrowright" => "Right".into(),
        "up" | "arrowup" => "Up".into(),
        "down" | "arrowdown" => "Down".into(),
        "tab" => "Tab".into(),
        "enter" | "return" => "Enter".into(),
        "esc" | "escape" => "Escape".into(),
        "space" | "spacebar" => "Space".into(),
        "backspace" => "Backspace".into(),
        "delete" | "del" => "Delete".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "comma" => ",".into(),
        "period" => ".".into(),
        "plus" => "+".into(),
        "minus" => "-".into(),
        _ if lower.starts_with('f') && lower[1..].parse::<u8>().is_ok() => {
            lower.to_ascii_uppercase()
        }
        _ => t.to_owned(),
    }
}

fn parse_section_header(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    let after = rest[end + 1..].trim();
    if after.is_empty() || after.starts_with('#') || after.starts_with(';') || after.starts_with("//") {
        Some(rest[..end].trim())
    } else {
        None
    }
}

fn qualify_key(section: &str, raw_key: &str) -> String {
    let key = raw_key.trim().to_ascii_lowercase();
    if section.is_empty() || key.contains('.') {
        key
    } else {
        format!("{section}.{key}")
    }
}

fn parse_key_value(line: &str) -> Option<(&str, ConfigValue)> {
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    if key.is_empty() {
        return None;
    }
    let raw = line[eq + 1..].trim();
    if raw.starts_with('"') {
        let (s, _) = parse_quoted(raw, '"')?;
        return Some((key, ConfigValue::Scalar(s)));
    }
    if raw.starts_with('\'') {
        let (s, _) = parse_quoted(raw, '\'')?;
        return Some((key, ConfigValue::Scalar(s)));
    }
    if raw.starts_with('[') {
        return Some((key, ConfigValue::List(parse_string_list(raw)?)));
    }
    let clean = strip_inline_comment(raw).trim().to_owned();
    Some((key, ConfigValue::Scalar(clean)))
}

fn parse_quoted(s: &str, quote: char) -> Option<(String, &str)> {
    let rest = s.strip_prefix(quote)?;
    let mut out = String::new();
    let mut chars = rest.char_indices();
    while let Some((idx, ch)) = chars.next() {
        if ch == '\\' && quote == '"' {
            if let Some((_, esc)) = chars.next() {
                match esc {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    other => out.push(other),
                }
            }
        } else if ch == quote {
            return Some((out, &rest[idx + ch.len_utf8()..]));
        } else {
            out.push(ch);
        }
    }
    None
}

fn parse_string_list(raw: &str) -> Option<Vec<String>> {
    let mut s = raw.strip_prefix('[')?.trim_start();
    let mut items = Vec::new();
    loop {
        if s.starts_with(']') {
            return Some(items);
        }
        if s.starts_with('"') {
            let (val, after) = parse_quoted(s, '"')?;
            items.push(val);
            s = after.trim_start();
        } else if s.starts_with('\'') {
            let (val, after) = parse_quoted(s, '\'')?;
            items.push(val);
            s = after.trim_start();
        } else {
            let end = s.find([',', ']'])?;
            let token = s[..end].trim();
            if !token.is_empty() {
                items.push(token.to_owned());
            }
            s = s[end..].trim_start();
        }
        if let Some(after_comma) = s.strip_prefix(',') {
            s = after_comma.trim_start();
        } else if s.starts_with(']') {
            return Some(items);
        } else {
            return None;
        }
    }
}

fn strip_inline_comment(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i - 1].is_ascii_whitespace()
            && (bytes[i] == b'#'
                || bytes[i] == b';'
                || (bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/')))
        {
            return &raw[..i];
        }
    }
    raw
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn parse_optional_color(val: &ConfigValue) -> Option<Rgba8> {
    val.as_scalar().and_then(parse_hex_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_config() {
        let cfg = NousConfig::default();
        let text = cfg.serialize();
        let parsed = NousConfig::parse(&text);
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn roundtrip_custom_config() {
        let mut cfg = NousConfig::default();
        cfg.tab_position = TabPosition::Left;
        cfg.toolbar_position = ToolbarPosition::Unified;
        cfg.sidebar_width = 275.0;
        cfg.sidebar_collapsed = true;
        cfg.density = Density::Compact;
        cfg.corner_radius = 14.0;
        cfg.theme_preset = ThemePreset::Nord;
        cfg.theme_overrides = ThemeOverrides {
            dark: Some(false),
            frame: Some([0x11, 0x22, 0x33, 0xff]),
            accent: Some([0x44, 0x55, 0x66, 0xaa]),
            ..ThemeOverrides::default()
        };
        cfg.ui_font = "Segoe UI".into();
        cfg.ui_font_size = 15.0;
        cfg.ui_scale = 1.25;
        cfg.toolbar_modules = ToolbarModules {
            home: false,
            ram_badge: false,
            ..ToolbarModules::default()
        };
        cfg.apply_perf_profile(PerformanceProfile::UltraLight);
        cfg.animations_enabled = true;
        cfg.cache_budget_mb = 16;
        cfg.max_image_decode_px = 768;
        cfg.home_url = "https://nous.dev".into();
        cfg.search_engine = "Custom".into();
        cfg.custom_search_url = Some("https://search.example/?q={query}".into());
        cfg.keybindings.set_for_action(
            ShortcutAction::OpenSettings,
            vec![Shortcut::parse("Ctrl+,").unwrap()],
        );
        cfg.keybindings.set_for_action(
            ShortcutAction::Reload,
            vec![
                Shortcut::parse("Ctrl+Shift+R").unwrap(),
                Shortcut::parse("F5").unwrap(),
            ],
        );

        let serialized = cfg.serialize();
        let parsed = NousConfig::parse(&serialized);
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn hex_colors_and_theme_overrides() {
        assert_eq!(parse_hex_color("#fff"), Some([255, 255, 255, 255]));
        assert_eq!(parse_hex_color("#1234"), Some([0x11, 0x22, 0x33, 0x44]));
        assert_eq!(parse_hex_color("#7C5CFF"), Some([0x7c, 0x5c, 0xff, 255]));
        assert_eq!(
            parse_hex_color("#22d3ee80"),
            Some([0x22, 0xd3, 0xee, 0x80])
        );
        assert_eq!(parse_hex_color("7c5cff"), None);
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("#zzzzzz"), None);

        let mut cfg = NousConfig::default();
        cfg.theme_preset = ThemePreset::CyberAmber;
        cfg.theme_overrides.accent = parse_hex_color("#112233");
        cfg.theme_overrides.dark = Some(false);
        let th = cfg.resolved_theme();
        assert!(!th.dark);
        assert_eq!(th.accent, [0x11, 0x22, 0x33, 255]);
        assert_eq!(th.frame, Theme::from_preset(ThemePreset::CyberAmber).frame);
    }

    #[test]
    fn unquoted_hex_comments_and_clamping() {
        let raw = r#"
            # leading comment
            tab_position = right
            toolbar_position = auto-hide
            corner_radius = 99.0 # trailing comment
            sidebar_width = 10.0
            ui_scale = 4.5
            ui_font_size = 2.0
            garbage line without equals
            unknown_key = something

            [theme.colors]
            frame = #010203 # inline comment after hex
            accent = #aabbccdd
        "#;
        let cfg = NousConfig::parse(raw);
        assert_eq!(cfg.tab_position, TabPosition::Right);
        assert_eq!(cfg.toolbar_position, ToolbarPosition::AutoHide);
        assert_eq!(cfg.corner_radius, 16.0);
        assert_eq!(cfg.sidebar_width, 120.0);
        assert_eq!(cfg.ui_scale, 2.0);
        assert_eq!(cfg.ui_font_size, 9.0);
        assert_eq!(cfg.theme_overrides.frame, Some([1, 2, 3, 255]));
        assert_eq!(cfg.theme_overrides.accent, Some([0xaa, 0xbb, 0xcc, 0xdd]));
    }

    #[test]
    fn shortcut_parsing_and_matching() {
        let s1 = Shortcut::parse("Ctrl+T").unwrap();
        assert!(s1.matches(true, false, false, "t"));
        assert!(!s1.matches(true, true, false, "t"));
        assert_eq!(s1.format_string(), "Ctrl+T");

        let s2 = Shortcut::parse("ctrl + shift + b").unwrap();
        assert_eq!(s2.format_string(), "Ctrl+Shift+B");
        assert!(s2.matches(true, true, false, "B"));

        let s3 = Shortcut::parse("Alt+Left").unwrap();
        assert!(s3.matches(false, false, true, "ArrowLeft"));
        assert_eq!(s3.format_string(), "Alt+Left");

        let s4 = Shortcut::parse("F5").unwrap();
        assert!(s4.matches(false, false, false, "f5"));
        let s5 = Shortcut::parse("F11").unwrap();
        assert!(s5.matches(false, false, false, "F11"));
        let s6 = Shortcut::parse("Ctrl+,").unwrap();
        assert!(s6.matches(true, false, false, ","));

        assert!(Shortcut::parse("").is_none());
        assert!(Shortcut::parse("Ctrl+").is_none());
        assert!(Shortcut::parse("Ctrl+Alt").is_none());

        let kb = KeyBindings::default();
        assert_eq!(kb.lookup(true, false, false, "t"), Some(ShortcutAction::NewTab));
        assert_eq!(
            kb.lookup(false, false, false, "F11"),
            Some(ShortcutAction::ToggleZenMode)
        );
        assert_eq!(
            kb.lookup(false, false, true, "ArrowLeft"),
            Some(ShortcutAction::Back)
        );
    }

    #[test]
    fn perf_profiles_and_file_persistence() {
        let mut cfg = NousConfig::default();
        cfg.apply_perf_profile(PerformanceProfile::UltraLight);
        assert!(!cfg.animations_enabled);
        assert!(!cfg.js_enabled);
        assert!(!cfg.images_enabled);
        assert_eq!(cfg.cache_budget_mb, 4);

        let dir = std::env::temp_dir().join(format!("nous-ui-test-{}", std::process::id()));
        let path = dir.join("nested").join("config.toml");
        cfg.save_to_path(&path).unwrap();
        let loaded = NousConfig::load_from_path(&path).unwrap();
        assert_eq!(loaded, cfg);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(NousConfig::default_config_path().is_some());
    }
}
