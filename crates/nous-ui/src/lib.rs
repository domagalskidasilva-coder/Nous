//! Interface do Nous: barra de abas (topo, baixo, barra lateral esquerda/direita,
//! modo Zen), barra de ferramentas configurável, omnibox, badges de telemetria e menu.
//! Tudo desenhado com Vello, usando o mesmo sistema de fontes das páginas.
//!
//! A interface não sabe nada de janelas: o app traduz eventos do sistema para as chamadas
//! daqui e executa as [`UiAction`]s que voltam.

pub mod config;
mod text_field;

use std::borrow::Cow;
use std::time::{Duration, Instant};

use nous_paint::{LayoutEngine, TextBrush, draw_text, to_color};
use parley::{FontFamily, FontFamilyName, FontWeight, GenericFamily, Layout, StyleProperty};
use vello::Scene;
use vello::kurbo::{
    Affine, Arc, BezPath, Circle, Line, Point, Rect, RoundedRect, RoundedRectRadii, Stroke, Vec2,
};
use vello::peniko::{Fill, Gradient};

pub use config::{
    Density, KeyBindings, NousConfig, PerformanceProfile, Shortcut, ShortcutAction, TabPosition,
    ThemeOverrides, ThemePreset, ToolbarModules, ToolbarPosition,
};
pub use text_field::TextField;

pub const TABSTRIP_H: f64 = 40.0;
pub const TOOLBAR_H: f64 = 44.0;
/// Altura total da interface acima do conteúdo da página no layout padrão.
pub const CHROME_H: f64 = TABSTRIP_H + TOOLBAR_H;
const CAPTION_W: f64 = 46.0;
const TAB_MAX_W: f64 = 240.0;
const TAB_MIN_W: f64 = 56.0;
const MENU_W: f64 = 290.0;
const MENU_ITEM_H: f64 = 34.0;

pub type Rgba8 = [u8; 4];

/// Cores da interface. O escuro segue a identidade NOUS (fundo #07080D, violeta #7C5CFF,
/// ciano #22D3EE, texto #F5F6FB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub dark: bool,
    pub frame: Rgba8,
    pub surface: Rgba8,
    pub hover: Rgba8,
    pub field: Rgba8,
    pub text: Rgba8,
    pub text_dim: Rgba8,
    pub separator: Rgba8,
    pub accent: Rgba8,
    pub accent2: Rgba8,
    pub danger: Rgba8,
}

impl Theme {
    pub const fn dark() -> Theme {
        Theme {
            dark: true,
            frame: [0x07, 0x08, 0x0d, 255],
            surface: [0x15, 0x16, 0x21, 255],
            hover: [0x24, 0x26, 0x36, 255],
            field: [0x0d, 0x0e, 0x16, 255],
            text: [0xf5, 0xf6, 0xfb, 255],
            text_dim: [0x9a, 0x9c, 0xb0, 255],
            separator: [0x2b, 0x2d, 0x3e, 255],
            accent: [0x7c, 0x5c, 0xff, 255],
            accent2: [0x22, 0xd3, 0xee, 255],
            danger: [0xe5, 0x48, 0x4d, 255],
        }
    }

    pub const fn light() -> Theme {
        Theme {
            dark: false,
            frame: [0xe7, 0xe7, 0xef, 255],
            surface: [0xff, 0xff, 0xff, 255],
            hover: [0xd8, 0xd8, 0xe5, 255],
            field: [0xf0, 0xf0, 0xf6, 255],
            text: [0x1d, 0x1d, 0x28, 255],
            text_dim: [0x5e, 0x60, 0x70, 255],
            separator: [0xcf, 0xcf, 0xdb, 255],
            accent: [0x7c, 0x5c, 0xff, 255],
            accent2: [0x0e, 0xa5, 0xc6, 255],
            danger: [0xe5, 0x48, 0x4d, 255],
        }
    }

    pub const fn from_preset(preset: ThemePreset) -> Theme {
        match preset {
            ThemePreset::NousDark | ThemePreset::Custom => Theme::dark(),
            ThemePreset::NousLight => Theme::light(),
            ThemePreset::MidnightOled => Theme {
                dark: true,
                frame: [0x00, 0x00, 0x00, 255],
                surface: [0x0a, 0x0a, 0x0f, 255],
                hover: [0x18, 0x18, 0x24, 255],
                field: [0x05, 0x05, 0x09, 255],
                text: [0xf4, 0xf5, 0xfa, 255],
                text_dim: [0x8b, 0x8d, 0xa3, 255],
                separator: [0x1f, 0x1f, 0x2e, 255],
                accent: [0x8b, 0x5c, 0xf6, 255],
                accent2: [0x06, 0xb6, 0xd4, 255],
                danger: [0xef, 0x44, 0x44, 255],
            },
            ThemePreset::Nord => Theme {
                dark: true,
                frame: [0x2e, 0x34, 0x40, 255],
                surface: [0x3b, 0x42, 0x52, 255],
                hover: [0x43, 0x4c, 0x5e, 255],
                field: [0x24, 0x29, 0x33, 255],
                text: [0xec, 0xef, 0xf4, 255],
                text_dim: [0xa5, 0xb1, 0xc2, 255],
                separator: [0x4c, 0x56, 0x6a, 255],
                accent: [0x88, 0xc0, 0xd0, 255],
                accent2: [0x81, 0xa1, 0xc1, 255],
                danger: [0xbf, 0x61, 0x6a, 255],
            },
            ThemePreset::CyberAmber => Theme {
                dark: true,
                frame: [0x0c, 0x09, 0x05, 255],
                surface: [0x17, 0x12, 0x0a, 255],
                hover: [0x28, 0x1f, 0x11, 255],
                field: [0x10, 0x0c, 0x06, 255],
                text: [0xfe, 0xf3, 0xc7, 255],
                text_dim: [0xb4, 0x9a, 0x67, 255],
                separator: [0x36, 0x29, 0x16, 255],
                accent: [0xf5, 0x9e, 0x0b, 255],
                accent2: [0xfb, 0x92, 0x3c, 255],
                danger: [0xef, 0x44, 0x44, 255],
            },
            ThemePreset::TerminalMono => Theme {
                dark: true,
                frame: [0x07, 0x0b, 0x07, 255],
                surface: [0x0e, 0x15, 0x0e, 255],
                hover: [0x18, 0x24, 0x18, 255],
                field: [0x0a, 0x0f, 0x0a, 255],
                text: [0xd8, 0xf8, 0xd8, 255],
                text_dim: [0x7c, 0xa8, 0x7c, 255],
                separator: [0x23, 0x36, 0x23, 255],
                accent: [0x22, 0xc5, 0x5e, 255],
                accent2: [0x4a, 0xde, 0x80, 255],
                danger: [0xf8, 0x71, 0x71, 255],
            },
        }
    }
}

/// O que a interface precisa saber para se desenhar.
pub struct TabView<'a> {
    pub title: &'a str,
    pub loading: bool,
}

pub struct UiModel<'a> {
    pub tabs: &'a [TabView<'a>],
    pub active: usize,
    pub url: &'a str,
    pub can_back: bool,
    pub can_forward: bool,
    pub loading: bool,
    pub maximized: bool,
    pub search_name: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    None,
    Tab(usize),
    TabClose(usize),
    NewTab,
    Back,
    Forward,
    Reload,
    Home,
    Omnibox,
    SidebarToggle,
    RamBadge,
    PerfBadge,
    Menu,
    MenuItem(usize),
    Minimize,
    Maximize,
    Close,
    DragArea,
    Content,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    NewTab,
    Memory,
    Settings,
    ToggleTheme,
    ToggleTabPosition,
    ToggleZenMode,
    CyclePerfProfile,
    ToggleSidebar,
    NextSearchEngine,
    About,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    SelectTab(usize),
    CloseTab(usize),
    NewTab,
    MoveTab { from: usize, to: usize },
    Back,
    Forward,
    Reload,
    Stop,
    Home,
    Navigate(String),
    ToggleSidebar,
    OpenMemory,
    OpenSettings,
    CyclePerfProfile,
    Minimize,
    ToggleMaximize,
    CloseWindow,
    DragWindow,
    Copy(String),
    Menu(MenuCommand),
}

/// Teclas que a interface entende (o app traduz do sistema).
#[derive(Clone, Debug, PartialEq)]
pub enum UiKey {
    Text(String),
    Backspace,
    Delete,
    Left { shift: bool, word: bool },
    Right { shift: bool, word: bool },
    Home { shift: bool },
    End { shift: bool },
    Enter,
    Escape,
    SelectAll,
    Copy,
    Cut,
    Paste(String),
}

struct TabDrag {
    index: usize,
    start_x: f64,
    start_y: f64,
    moved: bool,
}

struct LayoutGeom {
    content: Rect,
    tabstrip: Option<Rect>,
    toolbar: Option<Rect>,
    tabs_vertical: bool,
    unified: bool,
    captions: [(Hit, Rect); 3],
    captions_visible: bool,
    tabs: Vec<Rect>,
    tab_closes: Vec<Option<(Point, f64)>>,
    new_tab: Option<Rect>,
    sidebar_toggle_in_sidebar: Option<Rect>,
    sidebar_header: Option<Rect>,
    tool_buttons: Vec<(Hit, Rect)>,
    omnibox: Option<Rect>,
    tab_axis_origin: f64,
    tab_axis_step: f64,
    progress_rect: Option<Rect>,
}

struct PaintCtx {
    th: Theme,
    s: f64,
    rad: f64,
    base_font: f32,
    tab_font: f32,
    ob_font: f32,
    badge_font: f32,
    time: f64,
}

pub struct Ui {
    pub theme: Theme,
    pub omnibox: TextField,
    config: NousConfig,
    prev_non_zen_tab_pos: TabPosition,
    ram_mb: Option<f32>,
    blocked_count: u64,
    auto_hide_revealed: bool,
    size: (f64, f64),
    hover: Hit,
    pressed: Hit,
    menu_open: bool,
    drag: Option<TabDrag>,
    last_click: Option<(Instant, Hit)>,
    pending_click_x: Option<f64>,
    started: Instant,
}

impl Ui {
    pub fn new(theme: Theme) -> Self {
        let mut config = NousConfig::default();
        config.theme_preset = if theme.dark {
            ThemePreset::NousDark
        } else {
            ThemePreset::NousLight
        };
        let mut ui = Self::with_config(config);
        ui.theme = theme;
        ui
    }

    pub fn with_config(mut config: NousConfig) -> Self {
        config.clamp_fields();
        let theme = config.resolved_theme();
        let prev_non_zen = if config.tab_position == TabPosition::Hidden {
            TabPosition::Top
        } else {
            config.tab_position
        };
        Ui {
            theme,
            omnibox: TextField::default(),
            config,
            prev_non_zen_tab_pos: prev_non_zen,
            ram_mb: None,
            blocked_count: 0,
            auto_hide_revealed: false,
            size: (1280.0, 800.0),
            hover: Hit::None,
            pressed: Hit::None,
            menu_open: false,
            drag: None,
            last_click: None,
            pending_click_x: None,
            started: Instant::now(),
        }
    }

    pub fn config(&self) -> &NousConfig {
        &self.config
    }

    pub fn apply_config(&mut self, mut config: NousConfig) {
        config.clamp_fields();
        if config.tab_position != TabPosition::Hidden {
            self.prev_non_zen_tab_pos = config.tab_position;
        }
        self.theme = config.resolved_theme();
        self.config = config;
        if !self.uses_auto_hide() {
            self.auto_hide_revealed = false;
        }
    }

    pub fn set_telemetry(&mut self, ram_mb: Option<f32>, blocked_count: u64) {
        self.ram_mb = ram_mb;
        self.blocked_count = blocked_count;
    }

    /// Tamanho lógico da janela.
    pub fn set_size(&mut self, w: f64, h: f64) {
        self.size = (w, h);
    }

    pub fn content_rect(&self) -> Rect {
        self.compute_content_rect()
    }

    pub fn menu_open(&self) -> bool {
        self.menu_open
    }

    pub fn close_menu(&mut self) {
        self.menu_open = false;
    }

    /// A interface precisa de quadros contínuos (animação de carregamento)?
    pub fn animating(&self, m: &UiModel) -> bool {
        self.config.animations_enabled && (m.loading || m.tabs.iter().any(|t| t.loading))
    }

    pub fn focus_omnibox(&mut self, url: &str) {
        self.omnibox.focus_with(url);
        self.menu_open = false;
        if self.uses_auto_hide() {
            self.auto_hide_revealed = true;
        }
    }

    // ------------------------------------------------------------ geometria

    fn scale(&self) -> f64 {
        self.config.ui_scale.clamp(0.75, 2.0)
    }

    fn uses_auto_hide(&self) -> bool {
        self.config.tab_position == TabPosition::Hidden
            || self.config.toolbar_position == ToolbarPosition::AutoHide
    }

    fn toolbar_visible(&self) -> bool {
        if self.uses_auto_hide() {
            self.auto_hide_revealed || self.omnibox.focused || self.menu_open
        } else {
            true
        }
    }

    fn compute_content_rect(&self) -> Rect {
        let (w, h) = (self.size.0.max(0.0), self.size.1.max(0.0));
        let s = self.scale();
        let d = self.config.density;
        let ts_h = d.tabstrip_height() * s;
        let tb_h = d.toolbar_height() * s;
        let uni_h = ts_h.max(tb_h);
        let sb_w = (self.config.effective_sidebar_width() * s).min(w);

        match self.config.tab_position {
            TabPosition::Hidden => Rect::new(0.0, 0.0, w, h),
            TabPosition::Top => match self.config.toolbar_position {
                ToolbarPosition::Top => Rect::new(0.0, (ts_h + tb_h).min(h), w, h),
                ToolbarPosition::Unified => Rect::new(0.0, uni_h.min(h), w, h),
                ToolbarPosition::Bottom => {
                    let y0 = ts_h.min(h);
                    Rect::new(0.0, y0, w, (h - tb_h).max(y0))
                }
                ToolbarPosition::AutoHide => Rect::new(0.0, ts_h.min(h), w, h),
            },
            TabPosition::Bottom => match self.config.toolbar_position {
                ToolbarPosition::Top => {
                    let y0 = tb_h.min(h);
                    Rect::new(0.0, y0, w, (h - ts_h).max(y0))
                }
                ToolbarPosition::Unified => Rect::new(0.0, 0.0, w, (h - uni_h).max(0.0)),
                ToolbarPosition::Bottom => Rect::new(0.0, 0.0, w, (h - ts_h - tb_h).max(0.0)),
                ToolbarPosition::AutoHide => Rect::new(0.0, 0.0, w, (h - ts_h).max(0.0)),
            },
            TabPosition::Left => match self.config.toolbar_position {
                ToolbarPosition::Top | ToolbarPosition::Unified => {
                    Rect::new(sb_w, tb_h.min(h), w, h)
                }
                ToolbarPosition::Bottom => Rect::new(sb_w, 0.0, w, (h - tb_h).max(0.0)),
                ToolbarPosition::AutoHide => Rect::new(sb_w, 0.0, w, h),
            },
            TabPosition::Right => match self.config.toolbar_position {
                ToolbarPosition::Top | ToolbarPosition::Unified => {
                    Rect::new(0.0, tb_h.min(h), (w - sb_w).max(0.0), h)
                }
                ToolbarPosition::Bottom => {
                    Rect::new(0.0, 0.0, (w - sb_w).max(0.0), (h - tb_h).max(0.0))
                }
                ToolbarPosition::AutoHide => Rect::new(0.0, 0.0, (w - sb_w).max(0.0), h),
            },
        }
    }

    fn ram_badge_label(&self) -> String {
        match (self.ram_mb, self.blocked_count) {
            (Some(mb), 0) => format!("{mb:.0} MB"),
            (Some(mb), blk) => format!("{mb:.0} MB · {blk}"),
            (None, 0) => "RAM".into(),
            (None, blk) => format!("{blk} blk"),
        }
    }

    fn ram_badge_width(&self, scale: f64) -> f64 {
        let len = self.ram_badge_label().chars().count() as f64;
        (len * 6.8 + 20.0).max(54.0) * scale
    }

    fn omnibox_text_inset(&self) -> f64 {
        16.0 * self.scale()
    }

    fn compute_tabstrip_rect(&self, unified: bool) -> Option<Rect> {
        let (w, h) = (self.size.0.max(0.0), self.size.1.max(0.0));
        let s = self.scale();
        let d = self.config.density;
        let ts_h = d.tabstrip_height() * s;
        let tb_h = d.toolbar_height() * s;
        let uni_h = ts_h.max(tb_h);
        let sb_w = (self.config.effective_sidebar_width() * s).min(w);
        let side_y = match self.config.toolbar_position {
            ToolbarPosition::Top | ToolbarPosition::Unified => (tb_h.min(h), h),
            ToolbarPosition::Bottom => (0.0, (h - tb_h).max(0.0)),
            ToolbarPosition::AutoHide => (0.0, h),
        };
        match self.config.tab_position {
            TabPosition::Hidden => None,
            TabPosition::Top => Some(Rect::new(0.0, 0.0, w, if unified { uni_h } else { ts_h }.min(h))),
            TabPosition::Bottom => Some(Rect::new(
                0.0,
                (h - if unified { uni_h } else { ts_h }).max(0.0),
                w,
                h,
            )),
            TabPosition::Left => Some(Rect::new(0.0, side_y.0, sb_w, side_y.1)),
            TabPosition::Right => Some(Rect::new((w - sb_w).max(0.0), side_y.0, w, side_y.1)),
        }
    }

    fn compute_toolbar_rect(&self, tabstrip: Option<Rect>, unified: bool) -> Option<Rect> {
        if !self.toolbar_visible() {
            return None;
        }
        if unified {
            return tabstrip;
        }
        let (w, h) = (self.size.0.max(0.0), self.size.1.max(0.0));
        let s = self.scale();
        let d = self.config.density;
        let ts_h = d.tabstrip_height() * s;
        let tb_h = d.toolbar_height() * s;
        let sb_w = (self.config.effective_sidebar_width() * s).min(w);
        Some(match self.config.tab_position {
            TabPosition::Hidden => Rect::new(0.0, 0.0, w, tb_h.min(h)),
            TabPosition::Top => match self.config.toolbar_position {
                ToolbarPosition::Top | ToolbarPosition::AutoHide => {
                    Rect::new(0.0, ts_h, w, (ts_h + tb_h).min(h))
                }
                ToolbarPosition::Bottom => Rect::new(0.0, (h - tb_h).max(ts_h), w, h),
                ToolbarPosition::Unified => return tabstrip,
            },
            TabPosition::Bottom => match self.config.toolbar_position {
                ToolbarPosition::Top | ToolbarPosition::AutoHide => Rect::new(0.0, 0.0, w, tb_h.min(h)),
                ToolbarPosition::Bottom => {
                    Rect::new(0.0, (h - ts_h - tb_h).max(0.0), w, (h - ts_h).max(0.0))
                }
                ToolbarPosition::Unified => return tabstrip,
            },
            TabPosition::Left | TabPosition::Right => match self.config.toolbar_position {
                ToolbarPosition::Top | ToolbarPosition::Unified => Rect::new(0.0, 0.0, w, tb_h.min(h)),
                ToolbarPosition::Bottom => Rect::new(0.0, (h - tb_h).max(0.0), w, h),
                ToolbarPosition::AutoHide => {
                    let x0 = if self.config.tab_position == TabPosition::Left { sb_w } else { 0.0 };
                    let x1 = if self.config.tab_position == TabPosition::Right {
                        (w - sb_w).max(0.0)
                    } else {
                        w
                    };
                    Rect::new(x0, 0.0, x1, tb_h.min(h))
                }
            },
        })
    }

    fn compute_captions(
        &self,
        tabstrip: Option<Rect>,
        toolbar: Option<Rect>,
        unified: bool,
    ) -> (Option<Rect>, bool, [(Hit, Rect); 3]) {
        let (w, h) = (self.size.0.max(0.0), self.size.1.max(0.0));
        let s = self.scale();
        let ts_h = self.config.density.tabstrip_height() * s;
        let cap_w = CAPTION_W * s;

        let (host, visible) = if self.config.tab_position == TabPosition::Hidden {
            (toolbar, toolbar.is_some())
        } else if self.config.tab_position == TabPosition::Top && !unified {
            (tabstrip, true)
        } else if let Some(tb) = toolbar {
            (Some(tb), true)
        } else if self.config.tab_position == TabPosition::Bottom {
            (tabstrip, true)
        } else {
            let cap_h = (32.0 * s).min(h);
            (Some(Rect::new((w - cap_w * 3.0).max(0.0), 0.0, w, cap_h)), true)
        };

        let area = host.unwrap_or(Rect::ZERO);
        let cap_h = (ts_h - 6.0 * s).clamp(24.0 * s, area.height().max(24.0 * s));
        let y0 = area.y0;
        let y1 = (y0 + cap_h).min(area.y1.max(y0 + cap_h));
        let rects = [
            (Hit::Minimize, Rect::new(w - 3.0 * cap_w, y0, w - 2.0 * cap_w, y1)),
            (Hit::Maximize, Rect::new(w - 2.0 * cap_w, y0, w - cap_w, y1)),
            (Hit::Close, Rect::new(w - cap_w, y0, w, y1)),
        ];
        (host, visible, rects)
    }

    fn push_left_tool_buttons(&self, bar: Rect, pad: f64, btn: f64, gap: f64, out: &mut Vec<(Hit, Rect)>) -> f64 {
        let cy = bar.center().y;
        let mut x = bar.x0 + pad;
        let tm = &self.config.toolbar_modules;
        for (enabled, hit) in [
            (tm.sidebar_toggle, Hit::SidebarToggle),
            (tm.back, Hit::Back),
            (tm.forward, Hit::Forward),
            (tm.reload, Hit::Reload),
            (tm.home, Hit::Home),
        ] {
            if enabled {
                out.push((hit, Rect::new(x, cy - btn / 2.0, x + btn, cy + btn / 2.0)));
                x += btn + gap;
            }
        }
        x
    }

    fn push_right_tool_buttons(
        &self,
        bar: Rect,
        right_edge: f64,
        btn: f64,
        gap: f64,
        s: f64,
        out: &mut Vec<(Hit, Rect)>,
    ) -> f64 {
        let cy = bar.center().y;
        let mut x = right_edge;
        let tm = &self.config.toolbar_modules;
        if tm.menu {
            out.push((Hit::Menu, Rect::new(x - btn, cy - btn / 2.0, x, cy + btn / 2.0)));
            x -= btn + gap;
        }
        let ph = (24.0 * s).min((bar.height() - 8.0 * s).max(16.0 * s));
        if tm.perf_badge {
            let pw = 48.0 * s;
            out.push((Hit::PerfBadge, Rect::new(x - pw, cy - ph / 2.0, x, cy + ph / 2.0)));
            x -= pw + gap;
        }
        if tm.ram_badge {
            let rw = self.ram_badge_width(s);
            out.push((Hit::RamBadge, Rect::new(x - rw, cy - ph / 2.0, x, cy + ph / 2.0)));
            x -= rw + gap;
        }
        x
    }

    fn layout(&self, tab_count: usize) -> LayoutGeom {
        let (w, h) = (self.size.0.max(0.0), self.size.1.max(0.0));
        let s = self.scale();
        let d = self.config.density;
        let pad = d.padding() * s;
        let btn = d.button_size() * s;
        let gap = 4.0 * s;
        let cap_w = CAPTION_W * s;

        let unified = self.config.tab_position.is_horizontal()
            && self.config.toolbar_position == ToolbarPosition::Unified;
        let tabs_vertical = self.config.tab_position.is_vertical();
        let tabstrip = self.compute_tabstrip_rect(unified);
        let toolbar = self.compute_toolbar_rect(tabstrip, unified);
        let (cap_host, captions_visible, captions) = self.compute_captions(tabstrip, toolbar, unified);

        let mut geom = LayoutGeom {
            content: self.compute_content_rect(),
            tabstrip,
            toolbar,
            tabs_vertical,
            unified,
            captions,
            captions_visible,
            tabs: Vec::with_capacity(tab_count),
            tab_closes: Vec::with_capacity(tab_count),
            new_tab: None,
            sidebar_toggle_in_sidebar: None,
            sidebar_header: None,
            tool_buttons: Vec::new(),
            omnibox: None,
            tab_axis_origin: 0.0,
            tab_axis_step: 0.0,
            progress_rect: toolbar
                .map(|tb| Rect::new(tb.x0, tb.y1 - 2.5, tb.x1, tb.y1 - 0.5))
                .or_else(|| Some(Rect::new(0.0, 0.0, w, 2.5))),
        };

        if unified {
            if let Some(bar) = tabstrip {
                self.layout_unified_bar(&mut geom, bar, tab_count, pad, btn, gap, cap_w, s);
            }
            return geom;
        }

        if let Some(tb) = toolbar {
            let cap_in_tb = captions_visible && Self::same_y_band(cap_host, tb);
            let right_edge = tb.x1 - pad - if cap_in_tb { 3.0 * cap_w + gap } else { 0.0 };
            let left_x = self.push_left_tool_buttons(tb, pad, btn, gap, &mut geom.tool_buttons);
            let right_x = self.push_right_tool_buttons(tb, right_edge, btn, gap, s, &mut geom.tool_buttons);
            if self.config.toolbar_modules.omnibox && right_x - left_x > 40.0 * s {
                let ob_h = (tb.height() - 10.0 * s).max(24.0 * s);
                let cy = tb.center().y;
                geom.omnibox = Some(Rect::new(
                    left_x + 4.0 * s,
                    cy - ob_h / 2.0,
                    (right_x - 4.0 * s).max(left_x + 44.0 * s),
                    cy + ob_h / 2.0,
                ));
            }
        }

        if let Some(ts) = tabstrip {
            if tabs_vertical {
                self.layout_vertical_tabs(&mut geom, ts, tab_count, pad, btn, s);
            } else {
                let cap_in_ts = captions_visible && Self::same_y_band(cap_host, ts);
                let right_reserve = if cap_in_ts { 3.0 * cap_w + 12.0 * s } else { 12.0 * s };
                self.layout_horizontal_tabs(&mut geom, ts, tab_count, pad, right_reserve, s);
            }
        }
        let _ = h;
        geom
    }

    fn same_y_band(host: Option<Rect>, target: Rect) -> bool {
        host.is_some_and(|cr| (cr.y0 - target.y0).abs() < 1.0 && (cr.y1 - target.y1).abs() < 1.0)
    }

    fn layout_unified_bar(
        &self,
        geom: &mut LayoutGeom,
        bar: Rect,
        tab_count: usize,
        pad: f64,
        btn: f64,
        gap: f64,
        cap_w: f64,
        s: f64,
    ) {
        let cy = bar.center().y;
        let mut left_x = self.push_left_tool_buttons(bar, pad, btn, gap, &mut geom.tool_buttons);
        let right_x = self.push_right_tool_buttons(
            bar,
            bar.x1 - (3.0 * cap_w + pad),
            btn,
            gap,
            s,
            &mut geom.tool_buttons,
        );
        let avail_mid = (right_x - left_x).max(0.0);
        if self.config.toolbar_modules.omnibox && avail_mid > 140.0 * s {
            let ob_w = (avail_mid * 0.38).clamp(130.0 * s, 320.0 * s);
            let ob_h = (bar.height() - 10.0 * s).max(24.0 * s);
            geom.omnibox = Some(Rect::new(left_x, cy - ob_h / 2.0, left_x + ob_w, cy + ob_h / 2.0));
            left_x += ob_w + pad;
        }
        let tabs_avail = (right_x - left_x - 36.0 * s).max(0.0);
        let tw = (tabs_avail / tab_count.max(1) as f64).clamp(TAB_MIN_W * s, TAB_MAX_W * s);
        geom.tab_axis_origin = left_x;
        geom.tab_axis_step = tw;
        let (top_pad, bot_pad) = self.horizontal_tab_pads(s, bar.height());
        let tab_y0 = bar.y0 + top_pad;
        let tab_y1 = (bar.y1 - bot_pad).max(tab_y0);
        for i in 0..tab_count {
            let x0 = left_x + i as f64 * tw;
            let x1 = (x0 + tw).min(right_x).max(x0);
            let r = Rect::new(x0, tab_y0, x1, tab_y1);
            geom.tab_closes.push(Some((Point::new((r.x1 - 16.0 * s).max(r.x0), r.center().y), 9.0 * s)));
            geom.tabs.push(r);
        }
        let nt_x = (left_x + tab_count as f64 * tw + 6.0 * s).min(right_x - 30.0 * s);
        if nt_x + 28.0 * s <= right_x {
            geom.new_tab = Some(Rect::new(nt_x, cy - 15.0 * s, nt_x + 30.0 * s, cy + 15.0 * s));
        }
    }

    fn layout_horizontal_tabs(
        &self,
        geom: &mut LayoutGeom,
        ts: Rect,
        tab_count: usize,
        pad: f64,
        right_reserve: f64,
        s: f64,
    ) {
        let x0 = ts.x0 + pad;
        let avail = (ts.width() - pad - 44.0 * s - right_reserve).max(TAB_MIN_W * s);
        let tw = (avail / tab_count.max(1) as f64).clamp(TAB_MIN_W * s, TAB_MAX_W * s);
        geom.tab_axis_origin = x0;
        geom.tab_axis_step = tw;
        let (top_pad, bot_pad) = self.horizontal_tab_pads(s, ts.height());
        let tab_y0 = ts.y0 + top_pad;
        let tab_y1 = (ts.y1 - bot_pad).max(tab_y0);
        for i in 0..tab_count {
            let tx0 = x0 + i as f64 * tw;
            let r = Rect::new(tx0, tab_y0, tx0 + tw, tab_y1);
            geom.tab_closes.push(Some((Point::new(r.x1 - 18.0 * s, r.center().y), 9.0 * s)));
            geom.tabs.push(r);
        }
        let nt_x = (x0 + tab_count as f64 * tw + 6.0 * s).min(ts.x1 - right_reserve - 30.0 * s);
        let nt_cy = ts.center().y;
        geom.new_tab = Some(Rect::new(nt_x, nt_cy - 15.0 * s, nt_x + 30.0 * s, nt_cy + 15.0 * s));
    }

    fn layout_vertical_tabs(
        &self,
        geom: &mut LayoutGeom,
        ts: Rect,
        tab_count: usize,
        pad: f64,
        btn: f64,
        s: f64,
    ) {
        let row_h = self.config.density.sidebar_row_height() * s;
        let hdr = Rect::new(ts.x0, ts.y0, ts.x1, (ts.y0 + row_h).min(ts.y1));
        geom.sidebar_header = Some(hdr);
        geom.sidebar_toggle_in_sidebar = Some(if self.config.sidebar_collapsed {
            let c = hdr.center();
            Rect::new(c.x - btn / 2.0, c.y - btn / 2.0, c.x + btn / 2.0, c.y + btn / 2.0)
        } else {
            Rect::new(
                hdr.x1 - pad - btn,
                hdr.center().y - btn / 2.0,
                hdr.x1 - pad,
                hdr.center().y + btn / 2.0,
            )
        });
        let list_y0 = hdr.y1 + 4.0 * s;
        geom.tab_axis_origin = list_y0;
        geom.tab_axis_step = row_h;
        let inset = 6.0 * s;
        for i in 0..tab_count {
            let y0 = list_y0 + i as f64 * row_h;
            if y0 >= ts.y1 - row_h {
                break;
            }
            let r = Rect::new(ts.x0 + inset, y0, ts.x1 - inset, (y0 + row_h - 4.0 * s).min(ts.y1));
            let close = (!self.config.sidebar_collapsed)
                .then(|| (Point::new(r.x1 - 14.0 * s, r.center().y), 9.0 * s));
            geom.tabs.push(r);
            geom.tab_closes.push(close);
        }
        let nt_y0 = (list_y0 + geom.tabs.len() as f64 * row_h + 2.0 * s).min((ts.y1 - row_h).max(list_y0));
        if nt_y0 + 24.0 * s <= ts.y1 {
            geom.new_tab = Some(Rect::new(
                ts.x0 + inset,
                nt_y0,
                ts.x1 - inset,
                (nt_y0 + row_h - 4.0 * s).min(ts.y1),
            ));
        }
    }

    fn horizontal_tab_pads(&self, s: f64, band_h: f64) -> (f64, f64) {
        let pad = (6.0 * s).min(band_h.max(0.0));
        if self.config.tab_position == TabPosition::Bottom {
            (0.0, pad)
        } else {
            (pad, 0.0)
        }
    }

    fn menu_items(&self, m: &UiModel) -> Vec<(String, MenuCommand)> {
        let new_tab_sc = self
            .config
            .keybindings
            .primary_shortcut(ShortcutAction::NewTab)
            .map(|s| s.format_string())
            .unwrap_or_else(|| "Ctrl+T".into());
        let tab_pos_label = match self.config.tab_position {
            TabPosition::Top => "Topo",
            TabPosition::Bottom => "Baixo",
            TabPosition::Left => "Esquerda",
            TabPosition::Right => "Direita",
            TabPosition::Hidden => "Ocultas (Zen)",
        };
        vec![
            (format!("{:<26} {new_tab_sc}", "Nova aba"), MenuCommand::NewTab),
            ("Alternar barra lateral".into(), MenuCommand::ToggleSidebar),
            (format!("Abas: {tab_pos_label}"), MenuCommand::ToggleTabPosition),
            (
                if self.config.tab_position == TabPosition::Hidden {
                    "Sair do modo Zen"
                } else {
                    "Modo Zen (ocultar interface)"
                }
                .into(),
                MenuCommand::ToggleZenMode,
            ),
            (
                format!("Desempenho: {}", self.config.perf_profile.display_name()),
                MenuCommand::CyclePerfProfile,
            ),
            (format!("Buscador: {}", m.search_name), MenuCommand::NextSearchEngine),
            (
                if self.theme.dark { "Tema claro" } else { "Tema escuro" }.into(),
                MenuCommand::ToggleTheme,
            ),
            ("Memória (nous://memoria)".into(), MenuCommand::Memory),
            ("Configurações (nous://config)".into(), MenuCommand::Settings),
            ("Sobre o Nous".into(), MenuCommand::About),
        ]
    }

    fn menu_rect(&self, geom: &LayoutGeom, count: usize) -> Rect {
        let s = self.scale();
        let (w, h) = (self.size.0.max(0.0), self.size.1.max(0.0));
        let margin = (8.0 * s).min(w * 0.5);
        let menu_w = (MENU_W * s).min((w - 2.0 * margin).max(0.0));
        let menu_h = 8.0 * s + count as f64 * MENU_ITEM_H * s;
        let min_x1 = margin + menu_w;
        let max_x1 = (w - margin).max(min_x1);
        let menu_btn = geom
            .tool_buttons
            .iter()
            .find(|(hit, _)| *hit == Hit::Menu)
            .map(|(_, r)| *r);
        let top_pad = (4.0 * s).min(h * 0.5);
        if let Some(btn) = menu_btn {
            let x1 = btn.x1.clamp(min_x1, max_x1);
            let x0 = (x1 - menu_w).max(0.0);
            if btn.center().y > h * 0.5 {
                let y1 = (btn.y0 - top_pad).clamp(top_pad, h.max(top_pad));
                let y0 = (y1 - menu_h).max(top_pad).min(y1);
                Rect::new(x0, y0, x1, y1)
            } else {
                let y0 = (btn.y1 + top_pad).clamp(top_pad, (h - menu_h).max(top_pad));
                let y1 = (y0 + menu_h).min(h.max(y0));
                Rect::new(x0, y0, x1, y1)
            }
        } else {
            let x1 = max_x1;
            let x0 = (x1 - menu_w).max(0.0);
            let y0 = (40.0 * s).min((h - menu_h).max(0.0));
            let y1 = (y0 + menu_h).min(h.max(y0));
            Rect::new(x0, y0, x1, y1)
        }
    }

    pub fn hit_test(&self, m: &UiModel, x: f64, y: f64) -> Hit {
        let p = Point::new(x, y);
        let geom = self.layout(m.tabs.len());
        if self.menu_open {
            let items = self.menu_items(m).len();
            let r = self.menu_rect(&geom, items);
            if r.contains(p) && items > 0 {
                let i = ((y - r.y0 - 4.0 * self.scale()) / (MENU_ITEM_H * self.scale()))
                    .floor()
                    .max(0.0) as usize;
                return Hit::MenuItem(i.min(items - 1));
            }
        }
        if geom.captions_visible {
            for (hit, r) in geom.captions {
                if r.contains(p) {
                    return hit;
                }
            }
        }
        if let Some(r) = geom.sidebar_toggle_in_sidebar
            && r.contains(p)
        {
            return Hit::SidebarToggle;
        }
        for (hit, r) in &geom.tool_buttons {
            if r.contains(p) {
                return *hit;
            }
        }
        if let Some(ob) = geom.omnibox
            && ob.contains(p)
        {
            return Hit::Omnibox;
        }
        for (i, r) in geom.tabs.iter().enumerate() {
            if r.contains(p) {
                if let Some(Some((c, rad))) = geom.tab_closes.get(i) {
                    let show_close = geom.tabs_vertical || r.width() > 90.0 * self.scale() || i == m.active;
                    if show_close && (p - *c).hypot() <= *rad {
                        return Hit::TabClose(i);
                    }
                }
                return Hit::Tab(i);
            }
        }
        if let Some(nt) = geom.new_tab
            && nt.contains(p)
        {
            return Hit::NewTab;
        }
        if let Some(ts) = geom.tabstrip
            && ts.contains(p)
        {
            return if geom.tabs_vertical { Hit::None } else { Hit::DragArea };
        }
        if let Some(tb) = geom.toolbar
            && tb.contains(p)
        {
            return if tb.y0 <= 1.0 { Hit::DragArea } else { Hit::None };
        }
        if geom.content.contains(p) { Hit::Content } else { Hit::None }
    }

    // ------------------------------------------------------------- eventos

    /// Movimento do mouse. Devolve (precisa redesenhar, ação).
    pub fn mouse_move(&mut self, m: &UiModel, x: f64, y: f64) -> (bool, Option<UiAction>) {
        let mut redraw = false;
        if self.uses_auto_hide() {
            let s = self.scale();
            let ts_offset = if self.config.tab_position == TabPosition::Top {
                self.config.density.tabstrip_height() * s
            } else {
                0.0
            };
            let reveal_edge = ts_offset + 8.0 * s;
            let hide_edge = ts_offset + self.config.density.toolbar_height() * s + 12.0 * s;
            let should_reveal = if self.auto_hide_revealed {
                y <= hide_edge || self.omnibox.focused || self.menu_open || self.drag.is_some()
            } else {
                y <= reveal_edge || self.omnibox.focused || self.menu_open
            };
            if should_reveal != self.auto_hide_revealed {
                self.auto_hide_revealed = should_reveal;
                redraw = true;
            }
        }

        let hit = self.hit_test(m, x, y);
        if hit != self.hover {
            redraw = true;
        }
        self.hover = hit;

        let mut action = None;
        let n = m.tabs.len();
        if self.drag.is_some() && n > 0 {
            let geom = self.layout(n);
            if let Some(drag) = &mut self.drag {
                if !drag.moved && ((x - drag.start_x).abs() > 5.0 || (y - drag.start_y).abs() > 5.0) {
                    drag.moved = true;
                }
                if drag.moved && geom.tab_axis_step > 0.0 {
                    let coord = if geom.tabs_vertical { y } else { x };
                    let target = (((coord - geom.tab_axis_origin) / geom.tab_axis_step)
                        .floor()
                        .max(0.0) as usize)
                        .min(n - 1);
                    if target != drag.index {
                        action = Some(UiAction::MoveTab {
                            from: drag.index,
                            to: target,
                        });
                        drag.index = target;
                        drag.start_x = x;
                        drag.start_y = y;
                        redraw = true;
                    }
                }
            }
        }
        (redraw, action)
    }

    pub fn mouse_leave(&mut self) -> bool {
        let mut changed = self.hover != Hit::None;
        self.hover = Hit::None;
        if self.auto_hide_revealed && !self.omnibox.focused && !self.menu_open {
            self.auto_hide_revealed = false;
            changed = true;
        }
        changed
    }

    pub fn mouse_down(&mut self, m: &UiModel, x: f64, y: f64) -> Option<UiAction> {
        let hit = self.hit_test(m, x, y);
        self.pressed = hit;
        let double = matches!(self.last_click, Some((t, h)) if h == hit && t.elapsed() < Duration::from_millis(400));
        self.last_click = Some((Instant::now(), hit));
        if self.menu_open && !matches!(hit, Hit::MenuItem(_) | Hit::Menu) {
            self.menu_open = false;
        }
        if hit != Hit::Omnibox && self.omnibox.focused {
            self.omnibox.blur();
        }
        match hit {
            Hit::Tab(i) => {
                self.drag = Some(TabDrag {
                    index: i,
                    start_x: x,
                    start_y: y,
                    moved: false,
                });
                Some(UiAction::SelectTab(i))
            }
            Hit::DragArea if double => Some(UiAction::ToggleMaximize),
            Hit::DragArea => Some(UiAction::DragWindow),
            Hit::Omnibox => {
                if !self.omnibox.focused {
                    self.omnibox.focus_with(m.url);
                } else if let Some(ob) = self.layout(m.tabs.len()).omnibox {
                    let local = x - ob.x0 - self.omnibox_text_inset();
                    self.pending_click_x = Some(local);
                }
                None
            }
            _ => None,
        }
    }

    pub fn mouse_up(&mut self, m: &UiModel, x: f64, y: f64) -> Option<UiAction> {
        let hit = self.hit_test(m, x, y);
        let pressed = std::mem::replace(&mut self.pressed, Hit::None);
        self.drag = None;
        if hit != pressed {
            return None;
        }
        match hit {
            Hit::TabClose(i) => Some(UiAction::CloseTab(i)),
            Hit::NewTab => Some(UiAction::NewTab),
            Hit::Back if m.can_back => Some(UiAction::Back),
            Hit::Forward if m.can_forward => Some(UiAction::Forward),
            Hit::Reload => Some(if m.loading { UiAction::Stop } else { UiAction::Reload }),
            Hit::Home => Some(UiAction::Home),
            Hit::SidebarToggle => {
                self.toggle_sidebar_state();
                Some(UiAction::ToggleSidebar)
            }
            Hit::RamBadge => Some(UiAction::OpenMemory),
            Hit::PerfBadge => {
                let next = self.config.perf_profile.next();
                self.config.apply_perf_profile(next);
                Some(UiAction::CyclePerfProfile)
            }
            Hit::Minimize => Some(UiAction::Minimize),
            Hit::Maximize => Some(UiAction::ToggleMaximize),
            Hit::Close => Some(UiAction::CloseWindow),
            Hit::Menu => {
                self.menu_open = !self.menu_open;
                None
            }
            Hit::MenuItem(i) => {
                self.menu_open = false;
                let cmd = self.menu_items(m).get(i).map(|(_, c)| *c)?;
                self.apply_menu_side_effects(cmd);
                Some(UiAction::Menu(cmd))
            }
            _ => None,
        }
    }

    fn toggle_sidebar_state(&mut self) {
        if self.config.tab_position.is_vertical() {
            self.config.sidebar_collapsed = !self.config.sidebar_collapsed;
        } else {
            self.config.tab_position = TabPosition::Left;
            self.config.sidebar_collapsed = false;
            self.prev_non_zen_tab_pos = TabPosition::Left;
        }
    }

    fn apply_menu_side_effects(&mut self, cmd: MenuCommand) {
        match cmd {
            MenuCommand::ToggleSidebar => self.toggle_sidebar_state(),
            MenuCommand::ToggleTabPosition => {
                let next = match self.config.tab_position {
                    TabPosition::Top => TabPosition::Left,
                    TabPosition::Left => TabPosition::Right,
                    TabPosition::Right => TabPosition::Bottom,
                    TabPosition::Bottom | TabPosition::Hidden => TabPosition::Top,
                };
                self.config.tab_position = next;
                self.prev_non_zen_tab_pos = next;
            }
            MenuCommand::ToggleZenMode => {
                if self.config.tab_position == TabPosition::Hidden {
                    self.config.tab_position = self.prev_non_zen_tab_pos;
                } else {
                    self.prev_non_zen_tab_pos = self.config.tab_position;
                    self.config.tab_position = TabPosition::Hidden;
                }
            }
            MenuCommand::CyclePerfProfile => {
                let next = self.config.perf_profile.next();
                self.config.apply_perf_profile(next);
            }
            _ => {}
        }
    }

    /// Clique do meio: fecha a aba (como nos navegadores conhecidos).
    pub fn middle_click(&mut self, m: &UiModel, x: f64, y: f64) -> Option<UiAction> {
        match self.hit_test(m, x, y) {
            Hit::Tab(i) | Hit::TabClose(i) => Some(UiAction::CloseTab(i)),
            _ => None,
        }
    }

    /// Tecla com a omnibox focada. `None` = a interface não usou a tecla.
    pub fn key(&mut self, key: UiKey) -> Option<Option<UiAction>> {
        if !self.omnibox.focused {
            return None;
        }
        let f = &mut self.omnibox;
        let action = match key {
            UiKey::Text(s) => {
                f.insert(&s);
                None
            }
            UiKey::Paste(s) => {
                f.insert(&s.replace(['\r', '\n'], " "));
                None
            }
            UiKey::Backspace => {
                f.backspace();
                None
            }
            UiKey::Delete => {
                f.delete();
                None
            }
            UiKey::Left { shift, word } => {
                f.move_left(shift, word);
                None
            }
            UiKey::Right { shift, word } => {
                f.move_right(shift, word);
                None
            }
            UiKey::Home { shift } => {
                f.home(shift);
                None
            }
            UiKey::End { shift } => {
                f.end(shift);
                None
            }
            UiKey::SelectAll => {
                f.select_all();
                None
            }
            UiKey::Copy => f.selected_text().map(|s| UiAction::Copy(s.to_owned())),
            UiKey::Cut => {
                let s = f.selected_text().map(str::to_owned);
                f.delete_selection();
                s.map(UiAction::Copy)
            }
            UiKey::Enter => {
                let text = f.text.trim().to_owned();
                f.blur();
                (!text.is_empty()).then_some(UiAction::Navigate(text))
            }
            UiKey::Escape => {
                f.blur();
                None
            }
        };
        Some(action)
    }

    // --------------------------------------------------------------- pintura

    fn text(
        &self,
        le: &mut LayoutEngine,
        text: &str,
        size: f32,
        weight: f32,
        color: Rgba8,
    ) -> Layout<TextBrush> {
        let mut b = le
            .layout_cx
            .ranged_builder(&mut le.font_cx, text, 1.0, true);
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
        if let Some(font) = self.config.effective_font() {
            let stack = format!("{font}, system-ui");
            b.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
                stack,
            ))));
        } else {
            b.push_default(StyleProperty::FontFamily(FontFamily::Single(
                FontFamilyName::Generic(GenericFamily::SystemUi),
            )));
        }
        b.push_default(StyleProperty::Brush(TextBrush { color, link: 0 }));
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout
    }

    fn fill(scene: &mut Scene, t: Affine, color: Rgba8, shape: &impl vello::kurbo::Shape) {
        scene.fill(Fill::NonZero, t, to_color(color), None, shape);
    }

    fn stroke(
        scene: &mut Scene,
        t: Affine,
        color: Rgba8,
        width: f64,
        shape: &impl vello::kurbo::Shape,
    ) {
        scene.stroke(&Stroke::new(width), t, to_color(color), None, shape);
    }

    fn with_alpha(c: Rgba8, a: u8) -> Rgba8 {
        [c[0], c[1], c[2], a]
    }

    pub fn paint(&mut self, scene: &mut Scene, le: &mut LayoutEngine, m: &UiModel, t: Affine) {
        let s = self.scale();
        let ctx = PaintCtx {
            th: self.theme,
            s,
            rad: self.config.effective_corner_radius() * s,
            base_font: self.config.ui_font_size * s as f32,
            tab_font: (self.config.ui_font_size - 0.5).max(9.0) * s as f32,
            ob_font: (self.config.ui_font_size + 1.0) * s as f32,
            badge_font: (self.config.ui_font_size - 1.5).max(8.5) * s as f32,
            time: self.started.elapsed().as_secs_f64(),
        };
        let geom = self.layout(m.tabs.len());

        self.paint_tabstrip_bg(scene, le, &geom, &ctx, t);
        self.paint_toolbar_bg(scene, &geom, &ctx, t);
        self.paint_tabs(scene, le, m, &geom, &ctx, t);
        self.paint_new_tab(scene, le, &geom, &ctx, t);
        self.paint_captions(scene, m, &geom, &ctx, t);
        self.paint_tool_buttons(scene, le, m, &geom, &ctx, t);
        self.paint_omnibox(scene, le, m, &geom, &ctx, t);
        self.paint_progress_bar(scene, m, &geom, &ctx, t);
        if self.menu_open {
            self.paint_menu(scene, le, m, &geom, &ctx, t);
        }
    }

    fn paint_tabstrip_bg(
        &self,
        scene: &mut Scene,
        le: &mut LayoutEngine,
        geom: &LayoutGeom,
        ctx: &PaintCtx,
        t: Affine,
    ) {
        let Some(ts) = geom.tabstrip else { return };
        if ts.width() <= 0.0 || ts.height() <= 0.0 {
            return;
        }
        Self::fill(scene, t, ctx.th.frame, &ts);
        if geom.tabs_vertical {
            let sep_x = if self.config.tab_position == TabPosition::Left {
                ts.x1 - 0.5
            } else {
                ts.x0 + 0.5
            };
            Self::stroke(scene, t, ctx.th.separator, 1.0, &Line::new((sep_x, ts.y0), (sep_x, ts.y1)));
            if let Some(hdr) = geom.sidebar_header {
                if !self.config.sidebar_collapsed {
                    let layout = self.text(le, "ABAS", ctx.badge_font, 600.0, ctx.th.text_dim);
                    let ty = hdr.center().y - layout.height() as f64 / 2.0;
                    draw_text(scene, &layout, t * Affine::translate((hdr.x0 + 12.0 * ctx.s, ty)), None);
                }
                if let Some(tog) = geom.sidebar_toggle_in_sidebar {
                    if self.hover == Hit::SidebarToggle {
                        let rr = RoundedRect::from_rect(tog, (ctx.rad * 0.7).min(8.0 * ctx.s));
                        Self::fill(scene, t, ctx.th.hover, &rr);
                    }
                    Self::paint_sidebar_icon(scene, t, tog.center(), ctx.s, ctx.th.text);
                }
            }
        } else if self.config.tab_position == TabPosition::Bottom && !geom.unified {
            Self::stroke(scene, t, ctx.th.separator, 1.0, &Line::new((ts.x0, ts.y0 + 0.5), (ts.x1, ts.y0 + 0.5)));
        }
    }

    fn paint_toolbar_bg(&self, scene: &mut Scene, geom: &LayoutGeom, ctx: &PaintCtx, t: Affine) {
        let Some(tb) = geom.toolbar else { return };
        if tb.width() <= 0.0 || tb.height() <= 0.0 {
            return;
        }
        if !geom.unified {
            Self::fill(scene, t, ctx.th.surface, &tb);
        }
        let sep_y = if tb.y0 > 0.0 && self.config.toolbar_position == ToolbarPosition::Bottom {
            tb.y0 + 0.5
        } else {
            tb.y1 - 0.5
        };
        Self::stroke(scene, t, ctx.th.separator, 1.0, &Line::new((tb.x0, sep_y), (tb.x1, sep_y)));
    }

    fn paint_tabs(
        &self,
        scene: &mut Scene,
        le: &mut LayoutEngine,
        m: &UiModel,
        geom: &LayoutGeom,
        ctx: &PaintCtx,
        t: Affine,
    ) {
        let n = geom.tabs.len();
        for i in 0..n {
            let tab = &m.tabs[i];
            let r = geom.tabs[i];
            let active = i == m.active;
            let hovered = matches!(self.hover, Hit::Tab(j) | Hit::TabClose(j) if j == i);
            let cy = r.center().y;

            if geom.tabs_vertical {
                let pill_r = ctx.rad.min(r.height() * 0.5);
                if active {
                    Self::fill(scene, t, ctx.th.surface, &RoundedRect::from_rect(r, pill_r));
                    let bar_y0 = r.y0 + 5.0 * ctx.s;
                    let bar = Rect::new(r.x0 + 1.5 * ctx.s, bar_y0, r.x0 + 4.5 * ctx.s, (r.y1 - 5.0 * ctx.s).max(bar_y0));
                    Self::fill(scene, t, ctx.th.accent, &RoundedRect::from_rect(bar, 1.5 * ctx.s));
                } else if hovered {
                    Self::fill(scene, t, ctx.th.hover, &RoundedRect::from_rect(r, pill_r));
                }
                let icon = if self.config.sidebar_collapsed {
                    r.center()
                } else {
                    Point::new(r.x0 + 18.0 * ctx.s, cy)
                };
                self.paint_tab_icon(scene, t, icon, ctx.s, tab.loading, ctx.time, ctx.th);
                if !self.config.sidebar_collapsed {
                    let close_pad = if hovered || active { 26.0 * ctx.s } else { 8.0 * ctx.s };
                    self.paint_tab_title(scene, le, r, r.x0 + 30.0 * ctx.s, (r.x1 - close_pad).max(r.x0 + 30.0 * ctx.s), tab.title, active, ctx, t);
                    if (hovered || active)
                        && let Some(Some((c, _))) = geom.tab_closes.get(i)
                    {
                        self.paint_close_glyph(scene, t, *c, ctx.s, self.hover == Hit::TabClose(i), ctx.th);
                    }
                }
            } else {
                if active {
                    let radii = if self.config.tab_position == TabPosition::Bottom {
                        RoundedRectRadii::new(0.0, 0.0, ctx.rad, ctx.rad)
                    } else {
                        RoundedRectRadii::new(ctx.rad, ctx.rad, 0.0, 0.0)
                    };
                    Self::fill(scene, t, ctx.th.surface, &RoundedRect::from_rect(r, radii));
                } else if hovered {
                    let hr = Rect::new(
                        r.x0 + 2.0 * ctx.s,
                        r.y0 + 2.0 * ctx.s,
                        (r.x1 - 2.0 * ctx.s).max(r.x0 + 2.0 * ctx.s),
                        (r.y1 - 4.0 * ctx.s).max(r.y0 + 2.0 * ctx.s),
                    );
                    Self::fill(scene, t, ctx.th.hover, &RoundedRect::from_rect(hr, (ctx.rad * 0.9).min(12.0 * ctx.s)));
                } else if i + 1 < n
                    && i + 1 != m.active
                    && !matches!(self.hover, Hit::Tab(j) | Hit::TabClose(j) if j == i + 1)
                {
                    Self::stroke(scene, t, ctx.th.separator, 1.0, &Line::new((r.x1, r.y0 + 8.0 * ctx.s), (r.x1, r.y1 - 8.0 * ctx.s)));
                }
                self.paint_tab_icon(scene, t, Point::new(r.x0 + 18.0 * ctx.s, cy), ctx.s, tab.loading, ctx.time, ctx.th);
                let show_close = r.width() > 90.0 * ctx.s || active;
                let text_x1 = if show_close { r.x1 - 30.0 * ctx.s } else { r.x1 - 8.0 * ctx.s };
                self.paint_tab_title(scene, le, r, r.x0 + 30.0 * ctx.s, text_x1.max(r.x0 + 30.0 * ctx.s), tab.title, active, ctx, t);
                if show_close && let Some(Some((c, _))) = geom.tab_closes.get(i) {
                    self.paint_close_glyph(scene, t, *c, ctx.s, self.hover == Hit::TabClose(i), ctx.th);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_tab_title(
        &self,
        scene: &mut Scene,
        le: &mut LayoutEngine,
        r: Rect,
        x0: f64,
        x1: f64,
        title: &str,
        active: bool,
        ctx: &PaintCtx,
        t: Affine,
    ) {
        let clip = Rect::new(x0, r.y0, x1, r.y1);
        if clip.width() <= 4.0 {
            return;
        }
        let color = if active { ctx.th.text } else { ctx.th.text_dim };
        let layout = self.text(le, title, ctx.tab_font, if active { 600.0 } else { 500.0 }, color);
        scene.push_clip_layer(Fill::NonZero, t, &clip);
        let ty = r.center().y - layout.height() as f64 / 2.0;
        draw_text(scene, &layout, t * Affine::translate((x0, ty)), None);
        scene.pop_layer();
    }

    fn paint_new_tab(
        &self,
        scene: &mut Scene,
        le: &mut LayoutEngine,
        geom: &LayoutGeom,
        ctx: &PaintCtx,
        t: Affine,
    ) {
        let Some(nt) = geom.new_tab else { return };
        let c = nt.center();
        if geom.tabs_vertical {
            if self.hover == Hit::NewTab {
                Self::fill(scene, t, ctx.th.hover, &RoundedRect::from_rect(nt, ctx.rad.min(nt.height() * 0.5)));
            }
            let plus_c = if self.config.sidebar_collapsed { c } else { Point::new(nt.x0 + 18.0 * ctx.s, c.y) };
            Self::paint_plus_glyph(scene, t, plus_c, ctx.s, ctx.th.text_dim);
            if !self.config.sidebar_collapsed {
                let layout = self.text(le, "Nova aba", ctx.tab_font, 500.0, ctx.th.text_dim);
                let ty = c.y - layout.height() as f64 / 2.0;
                draw_text(scene, &layout, t * Affine::translate((nt.x0 + 32.0 * ctx.s, ty)), None);
            }
        } else {
            if self.hover == Hit::NewTab {
                Self::fill(scene, t, ctx.th.hover, &Circle::new(c, 14.0 * ctx.s));
            }
            Self::paint_plus_glyph(scene, t, c, ctx.s, ctx.th.text);
        }
    }

    fn paint_captions(&self, scene: &mut Scene, m: &UiModel, geom: &LayoutGeom, ctx: &PaintCtx, t: Affine) {
        if !geom.captions_visible {
            return;
        }
        let s = ctx.s;
        for (hit, r) in geom.captions {
            let hovered = self.hover == hit;
            if hovered {
                Self::fill(scene, t, if hit == Hit::Close { ctx.th.danger } else { ctx.th.hover }, &r);
            }
            let color = if hovered && hit == Hit::Close { [255, 255, 255, 255] } else { ctx.th.text };
            let c = r.center();
            match hit {
                Hit::Minimize => Self::stroke(scene, t, color, 1.0, &Line::new((c.x - 5.0 * s, c.y), (c.x + 5.0 * s, c.y))),
                Hit::Maximize if m.maximized => {
                    Self::stroke(scene, t, color, 1.0, &Rect::new(c.x - 5.0 * s, c.y - 3.0 * s, c.x + 3.0 * s, c.y + 5.0 * s));
                    Self::stroke(scene, t, color, 1.0, &Line::new((c.x - 3.0 * s, c.y - 5.0 * s), (c.x + 5.0 * s, c.y - 5.0 * s)));
                    Self::stroke(scene, t, color, 1.0, &Line::new((c.x + 5.0 * s, c.y - 5.0 * s), (c.x + 5.0 * s, c.y + 3.0 * s)));
                }
                Hit::Maximize => Self::stroke(scene, t, color, 1.0, &Rect::new(c.x - 5.0 * s, c.y - 5.0 * s, c.x + 5.0 * s, c.y + 5.0 * s)),
                _ => {
                    Self::stroke(scene, t, color, 1.0, &Line::new((c.x - 5.0 * s, c.y - 5.0 * s), (c.x + 5.0 * s, c.y + 5.0 * s)));
                    Self::stroke(scene, t, color, 1.0, &Line::new((c.x - 5.0 * s, c.y + 5.0 * s), (c.x + 5.0 * s, c.y - 5.0 * s)));
                }
            }
        }
    }

    fn paint_tool_buttons(
        &self,
        scene: &mut Scene,
        le: &mut LayoutEngine,
        m: &UiModel,
        geom: &LayoutGeom,
        ctx: &PaintCtx,
        t: Affine,
    ) {
        let s = ctx.s;
        for (hit, r) in &geom.tool_buttons {
            let c = r.center();
            match *hit {
                Hit::Back | Hit::Forward | Hit::Reload | Hit::Home | Hit::SidebarToggle => {
                    let enabled = match *hit {
                        Hit::Back => m.can_back,
                        Hit::Forward => m.can_forward,
                        _ => true,
                    };
                    if enabled && self.hover == *hit {
                        Self::fill(scene, t, ctx.th.hover, &RoundedRect::from_rect(*r, (ctx.rad * 0.85).min(r.height() * 0.5)));
                    }
                    let color = if enabled { ctx.th.text } else { Self::with_alpha(ctx.th.text_dim, 110) };
                    Self::paint_nav_glyph(scene, t, *hit, c, s, m.loading, color);
                }
                Hit::RamBadge => {
                    let shape = RoundedRect::from_rect(*r, ctx.rad.min(r.height() * 0.5));
                    let bg = if self.hover == Hit::RamBadge { ctx.th.hover } else { ctx.th.field };
                    Self::fill(scene, t, bg, &shape);
                    Self::stroke(scene, t, ctx.th.separator, 1.0, &shape);
                    let layout = self.text(le, &self.ram_badge_label(), ctx.badge_font, 500.0, ctx.th.text_dim);
                    let tx = r.center().x - layout.full_width() as f64 / 2.0;
                    let ty = r.center().y - layout.height() as f64 / 2.0;
                    draw_text(scene, &layout, t * Affine::translate((tx, ty)), None);
                }
                Hit::PerfBadge => {
                    let shape = RoundedRect::from_rect(*r, ctx.rad.min(r.height() * 0.5));
                    let badge_color = match self.config.perf_profile {
                        PerformanceProfile::UltraLight => ctx.th.accent2,
                        PerformanceProfile::Balanced => ctx.th.accent,
                        PerformanceProfile::MaxSpeed => [0xf5, 0x9e, 0x0b, 255],
                    };
                    let bg = if self.hover == Hit::PerfBadge {
                        ctx.th.hover
                    } else {
                        Self::with_alpha(badge_color, if ctx.th.dark { 40 } else { 32 })
                    };
                    Self::fill(scene, t, bg, &shape);
                    Self::stroke(scene, t, Self::with_alpha(badge_color, 160), 1.0, &shape);
                    let layout = self.text(le, self.config.perf_profile.short_label(), ctx.badge_font, 600.0, badge_color);
                    let tx = r.center().x - layout.full_width() as f64 / 2.0;
                    let ty = r.center().y - layout.height() as f64 / 2.0;
                    draw_text(scene, &layout, t * Affine::translate((tx, ty)), None);
                }
                Hit::Menu => {
                    if self.hover == Hit::Menu || self.menu_open {
                        Self::fill(scene, t, ctx.th.hover, &Circle::new(c, (r.height() * 0.48).min(15.0 * s)));
                    }
                    for dy in [-5.0 * s, 0.0, 5.0 * s] {
                        Self::fill(scene, t, ctx.th.text, &Circle::new((c.x, c.y + dy), 1.6 * s));
                    }
                }
                _ => {}
            }
        }
    }

    fn paint_nav_glyph(scene: &mut Scene, t: Affine, hit: Hit, c: Point, s: f64, loading: bool, color: Rgba8) {
        match hit {
            Hit::Back | Hit::Forward => {
                let dir = if hit == Hit::Back { 1.0 } else { -1.0 };
                let mut p = BezPath::new();
                p.move_to((c.x + 6.0 * s * dir, c.y));
                p.line_to((c.x - 6.0 * s * dir, c.y));
                p.move_to((c.x - 1.0 * s * dir, c.y - 5.0 * s));
                p.line_to((c.x - 6.0 * s * dir, c.y));
                p.line_to((c.x - 1.0 * s * dir, c.y + 5.0 * s));
                Self::stroke(scene, t, color, 1.7, &p);
            }
            Hit::Reload if loading => {
                let d = 5.0 * s;
                Self::stroke(scene, t, color, 1.7, &Line::new((c.x - d, c.y - d), (c.x + d, c.y + d)));
                Self::stroke(scene, t, color, 1.7, &Line::new((c.x - d, c.y + d), (c.x + d, c.y - d)));
            }
            Hit::Reload => {
                Self::stroke(scene, t, color, 1.7, &Arc::new(c, (6.0 * s, 6.0 * s), -1.2, 5.0, 0.0));
                let tip = Point::new(c.x + 6.0 * s * (-1.2f64).cos(), c.y + 6.0 * s * (-1.2f64).sin());
                let mut p = BezPath::new();
                p.move_to((tip.x - 4.0 * s, tip.y - 2.0 * s));
                p.line_to(tip);
                p.line_to((tip.x + 0.5 * s, tip.y + 4.5 * s));
                Self::stroke(scene, t, color, 1.7, &p);
            }
            Hit::Home => {
                let mut p = BezPath::new();
                p.move_to((c.x - 6.0 * s, c.y));
                p.line_to((c.x, c.y - 5.5 * s));
                p.line_to((c.x + 6.0 * s, c.y));
                p.move_to((c.x - 4.2 * s, c.y - 1.0 * s));
                p.line_to((c.x - 4.2 * s, c.y + 5.5 * s));
                p.line_to((c.x + 4.2 * s, c.y + 5.5 * s));
                p.line_to((c.x + 4.2 * s, c.y - 1.0 * s));
                Self::stroke(scene, t, color, 1.6, &p);
            }
            Hit::SidebarToggle => Self::paint_sidebar_icon(scene, t, c, s, color),
            _ => {}
        }
    }

    fn paint_omnibox(
        &mut self,
        scene: &mut Scene,
        le: &mut LayoutEngine,
        m: &UiModel,
        geom: &LayoutGeom,
        ctx: &PaintCtx,
        t: Affine,
    ) {
        let Some(ob) = geom.omnibox else { return };
        let focused = self.omnibox.focused;
        let shape = RoundedRect::from_rect(ob, (ctx.rad * 2.0).min(ob.height() * 0.5));
        Self::fill(scene, t, if focused { ctx.th.surface } else { ctx.th.field }, &shape);
        if focused {
            Self::stroke(scene, t, ctx.th.accent, 2.0, &shape);
        } else if self.hover == Hit::Omnibox {
            Self::stroke(scene, t, ctx.th.separator, 1.0, &shape);
        }
        let tx = ob.x0 + self.omnibox_text_inset();
        let inner = Rect::new(ob.x0 + 10.0 * ctx.s, ob.y0, (ob.x1 - 12.0 * ctx.s).max(ob.x0 + 12.0 * ctx.s), ob.y1);
        if inner.width() <= 4.0 {
            return;
        }
        scene.push_clip_layer(Fill::NonZero, t, &inner);
        if focused {
            if let Some(local) = self.pending_click_x.take() {
                let idx = self.byte_index_at(le, local, ctx.ob_font);
                self.omnibox.set_cursor(idx, false);
            }
            let f = &self.omnibox;
            let x_of = |ui: &Ui, le: &mut LayoutEngine, idx: usize| -> f64 {
                if idx == 0 {
                    0.0
                } else {
                    ui.text(le, &f.text[..idx], ctx.ob_font, 400.0, ctx.th.text).full_width() as f64
                }
            };
            if let Some((a, b)) = f.selection() {
                let (xa, xb) = (x_of(self, le, a), x_of(self, le, b));
                let sel = Rect::new(tx + xa, ob.y0 + 7.0 * ctx.s, tx + xb, ob.y1 - 7.0 * ctx.s);
                Self::fill(scene, t, Self::with_alpha(ctx.th.accent, 90), &sel);
            }
            let layout = self.text(le, &f.text, ctx.ob_font, 400.0, ctx.th.text);
            let ty = ob.center().y - layout.height() as f64 / 2.0;
            draw_text(scene, &layout, t * Affine::translate((tx, ty)), None);
            let cx = tx + x_of(self, le, f.cursor);
            Self::stroke(scene, t, ctx.th.text, 1.2, &Line::new((cx, ob.y0 + 8.0 * ctx.s), (cx, ob.y1 - 8.0 * ctx.s)));
        } else {
            let (text, color) = if m.url.is_empty() {
                (format!("Pesquise com {} ou digite um endereço", m.search_name), ctx.th.text_dim)
            } else {
                (m.url.to_owned(), ctx.th.text)
            };
            let layout = self.text(le, &text, ctx.ob_font, 400.0, color);
            let ty = ob.center().y - layout.height() as f64 / 2.0;
            draw_text(scene, &layout, t * Affine::translate((tx, ty)), None);
        }
        scene.pop_layer();
    }

    fn paint_progress_bar(&self, scene: &mut Scene, m: &UiModel, geom: &LayoutGeom, ctx: &PaintCtx, t: Affine) {
        if !m.loading {
            return;
        }
        let Some(pr) = geom.progress_rect else { return };
        if pr.width() <= 2.0 {
            return;
        }
        let (x0, x1) = if self.config.animations_enabled {
            let phase = (ctx.time * 0.7).fract();
            let seg = pr.width() * 0.35;
            let start = pr.x0 - seg + phase * (pr.width() + seg);
            (start.max(pr.x0), (start + seg).min(pr.x1))
        } else {
            (pr.x0, pr.x0 + pr.width() * 0.65)
        };
        if x1 > x0 {
            let bar = Rect::new(x0, pr.y0, x1, pr.y1);
            let grad = Gradient::new_linear((x0, 0.0), (x1, 0.0))
                .with_stops([to_color(ctx.th.accent), to_color(ctx.th.accent2)]);
            scene.fill(Fill::NonZero, t, &grad, None, &bar);
        }
    }

    fn paint_menu(
        &self,
        scene: &mut Scene,
        le: &mut LayoutEngine,
        m: &UiModel,
        geom: &LayoutGeom,
        ctx: &PaintCtx,
        t: Affine,
    ) {
        let items = self.menu_items(m);
        let r = self.menu_rect(geom, items.len());
        if r.width() <= 8.0 * ctx.s || r.height() <= 8.0 * ctx.s {
            return;
        }
        let panel_r = ctx.rad.max(6.0 * ctx.s).min(14.0 * ctx.s);
        let shadow = RoundedRect::from_rect(r.inflate(2.0, 2.0) + Vec2::new(0.0, 3.0), panel_r + 2.0);
        Self::fill(scene, t, [0, 0, 0, if ctx.th.dark { 120 } else { 40 }], &shadow);
        let panel = RoundedRect::from_rect(r, panel_r);
        Self::fill(scene, t, ctx.th.surface, &panel);
        Self::stroke(scene, t, ctx.th.separator, 1.0, &panel);
        let item_h = MENU_ITEM_H * ctx.s;
        let row_r = (ctx.rad * 0.65).min(8.0 * ctx.s);
        scene.push_clip_layer(Fill::NonZero, t, &r);
        for (i, (label, _)) in items.iter().enumerate() {
            let y0 = r.y0 + 4.0 * ctx.s + i as f64 * item_h;
            let row = Rect::new(r.x0 + 4.0 * ctx.s, y0, r.x1 - 4.0 * ctx.s, y0 + item_h);
            if self.hover == Hit::MenuItem(i) {
                Self::fill(scene, t, ctx.th.hover, &RoundedRect::from_rect(row, row_r));
            }
            let layout = self.text(le, label, ctx.base_font, 400.0, ctx.th.text);
            let ty = row.center().y - layout.height() as f64 / 2.0;
            draw_text(scene, &layout, t * Affine::translate((row.x0 + 12.0 * ctx.s, ty)), None);
        }
        scene.pop_layer();
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_tab_icon(
        &self,
        scene: &mut Scene,
        t: Affine,
        icon: Point,
        s: f64,
        loading: bool,
        time: f64,
        th: Theme,
    ) {
        let r = 6.0 * s;
        if loading {
            let start = if self.config.animations_enabled { time * 6.0 } else { 0.0 };
            Self::stroke(scene, t, th.accent, 2.0, &Arc::new(icon, (r, r), start, 4.2, 0.0));
        } else {
            let grad = Gradient::new_linear((icon.x - r, icon.y - r), (icon.x + r, icon.y + r))
                .with_stops([to_color(th.accent), to_color(th.accent2)]);
            scene.fill(Fill::NonZero, t, &grad, None, &Circle::new(icon, r));
        }
    }

    fn paint_close_glyph(&self, scene: &mut Scene, t: Affine, c: Point, s: f64, hovered: bool, th: Theme) {
        if hovered {
            Self::fill(scene, t, th.hover, &Circle::new(c, 9.0 * s));
        }
        let d = 3.5 * s;
        Self::stroke(scene, t, th.text_dim, 1.4, &Line::new((c.x - d, c.y - d), (c.x + d, c.y + d)));
        Self::stroke(scene, t, th.text_dim, 1.4, &Line::new((c.x - d, c.y + d), (c.x + d, c.y - d)));
    }

    fn paint_plus_glyph(scene: &mut Scene, t: Affine, c: Point, s: f64, color: Rgba8) {
        Self::stroke(scene, t, color, 1.6, &Line::new((c.x - 6.0 * s, c.y), (c.x + 6.0 * s, c.y)));
        Self::stroke(scene, t, color, 1.6, &Line::new((c.x, c.y - 6.0 * s), (c.x, c.y + 6.0 * s)));
    }

    fn paint_sidebar_icon(scene: &mut Scene, t: Affine, c: Point, s: f64, color: Rgba8) {
        let (w, h) = (6.5 * s, 5.5 * s);
        Self::stroke(scene, t, color, 1.5, &RoundedRect::new(c.x - w, c.y - h, c.x + w, c.y + h, 2.0 * s));
        Self::stroke(scene, t, color, 1.5, &Line::new((c.x - 2.0 * s, c.y - h), (c.x - 2.0 * s, c.y + h)));
    }

    /// Índice (em bytes) do caractere mais próximo de `x` dentro do texto da omnibox.
    fn byte_index_at(&self, le: &mut LayoutEngine, x: f64, font_size: f32) -> usize {
        let text = &self.omnibox.text;
        let mut best = (0usize, f64::MAX);
        let mut boundaries: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        boundaries.push(text.len());
        for idx in boundaries {
            let w = if idx == 0 {
                0.0
            } else {
                self.text(le, &text[..idx], font_size, 400.0, self.theme.text).full_width() as f64
            };
            let d = (w - x).abs();
            if d < best.1 {
                best = (idx, d);
            }
        }
        best.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tabs() -> [TabView<'static>; 3] {
        [
            TabView { title: "Aba Um", loading: false },
            TabView { title: "Aba Dois", loading: true },
            TabView { title: "Aba Três", loading: false },
        ]
    }

    fn model<'a>(tabs: &'a [TabView<'a>], loading: bool) -> UiModel<'a> {
        UiModel {
            tabs,
            active: 0,
            url: "https://nous.dev",
            can_back: true,
            can_forward: false,
            loading,
            maximized: false,
            search_name: "DuckDuckGo",
        }
    }

    #[test]
    fn content_rect_all_layout_modes() {
        let mut ui = Ui::new(Theme::dark());
        ui.set_size(1280.0, 800.0);
        assert_eq!(ui.content_rect(), Rect::new(0.0, CHROME_H, 1280.0, 800.0));

        let mut cfg = NousConfig::default();

        cfg.tab_position = TabPosition::Top;
        cfg.toolbar_position = ToolbarPosition::Unified;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 44.0, 1280.0, 800.0));

        cfg.toolbar_position = ToolbarPosition::Bottom;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 40.0, 1280.0, 756.0));

        cfg.toolbar_position = ToolbarPosition::AutoHide;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 40.0, 1280.0, 800.0));

        cfg.tab_position = TabPosition::Bottom;
        cfg.toolbar_position = ToolbarPosition::Top;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 44.0, 1280.0, 760.0));

        cfg.toolbar_position = ToolbarPosition::Unified;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 0.0, 1280.0, 756.0));

        cfg.toolbar_position = ToolbarPosition::Bottom;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 0.0, 1280.0, 716.0));

        cfg.tab_position = TabPosition::Left;
        cfg.toolbar_position = ToolbarPosition::Top;
        cfg.sidebar_collapsed = false;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(220.0, 44.0, 1280.0, 800.0));

        cfg.sidebar_collapsed = true;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(52.0, 44.0, 1280.0, 800.0));

        cfg.tab_position = TabPosition::Right;
        cfg.sidebar_collapsed = false;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 44.0, 1060.0, 800.0));

        cfg.tab_position = TabPosition::Hidden;
        cfg.toolbar_position = ToolbarPosition::Top;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.content_rect(), Rect::new(0.0, 0.0, 1280.0, 800.0));

        cfg.tab_position = TabPosition::Top;
        cfg.toolbar_position = ToolbarPosition::Top;
        cfg.density = Density::Compact;
        cfg.ui_scale = 1.5;
        ui.apply_config(cfg);
        assert_eq!(ui.content_rect(), Rect::new(0.0, 105.0, 1280.0, 800.0));
    }

    #[test]
    fn hit_test_across_modes() {
        let tabs = sample_tabs();
        let m = model(&tabs, false);

        // Top + Top
        let mut cfg = NousConfig::default();
        let mut ui = Ui::with_config(cfg.clone());
        ui.set_size(1280.0, 800.0);
        ui.set_telemetry(Some(54.2), 3);

        assert_eq!(ui.hit_test(&m, 50.0, 20.0), Hit::Tab(0));
        assert_eq!(ui.hit_test(&m, 1280.0 - 10.0, 15.0), Hit::Close);
        assert_eq!(ui.hit_test(&m, 500.0, 200.0), Hit::Content);
        assert_eq!(ui.hit_test(&m, 24.0, 62.0), Hit::SidebarToggle);
        assert_eq!(ui.hit_test(&m, 60.0, 62.0), Hit::Back);
        assert_eq!(ui.hit_test(&m, 96.0, 62.0), Hit::Forward);
        assert_eq!(ui.hit_test(&m, 132.0, 62.0), Hit::Reload);
        assert_eq!(ui.hit_test(&m, 168.0, 62.0), Hit::Home);
        assert_eq!(ui.hit_test(&m, 500.0, 62.0), Hit::Omnibox);
        assert_eq!(ui.hit_test(&m, 1280.0 - 20.0, 62.0), Hit::Menu);

        // Top + Unified
        cfg.tab_position = TabPosition::Top;
        cfg.toolbar_position = ToolbarPosition::Unified;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.hit_test(&m, 24.0, 22.0), Hit::SidebarToggle);
        assert_eq!(ui.hit_test(&m, 1280.0 - 10.0, 22.0), Hit::Close);
        assert_eq!(ui.hit_test(&m, 500.0, 200.0), Hit::Content);

        // Left sidebar mode
        cfg.tab_position = TabPosition::Left;
        cfg.toolbar_position = ToolbarPosition::Top;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.hit_test(&m, 100.0, 95.0), Hit::Tab(0));
        assert_eq!(ui.hit_test(&m, 100.0, 129.0), Hit::Tab(1));
        assert_eq!(ui.hit_test(&m, 100.0, 200.0), Hit::NewTab);
        assert_eq!(ui.hit_test(&m, 100.0, 400.0), Hit::None);
        assert_eq!(ui.hit_test(&m, 300.0, 200.0), Hit::Content);

        // Right sidebar + Bottom toolbar
        cfg.tab_position = TabPosition::Right;
        cfg.toolbar_position = ToolbarPosition::Bottom;
        ui.apply_config(cfg.clone());
        assert_eq!(ui.hit_test(&m, 1150.0, 50.0), Hit::Tab(0));
        assert_eq!(ui.hit_test(&m, 500.0, 778.0), Hit::Omnibox);
        assert_eq!(ui.hit_test(&m, 500.0, 200.0), Hit::Content);

        // Zen mode + mouse_leave hide
        cfg.tab_position = TabPosition::Hidden;
        ui.apply_config(cfg);
        assert_eq!(ui.hit_test(&m, 500.0, 22.0), Hit::Content);
        let (redraw, _) = ui.mouse_move(&m, 500.0, 2.0);
        assert!(redraw);
        assert_eq!(ui.hit_test(&m, 500.0, 22.0), Hit::Omnibox);
        assert!(ui.mouse_leave());
        assert_eq!(ui.hit_test(&m, 500.0, 22.0), Hit::Content);
    }

    #[test]
    fn horizontal_and_vertical_tab_drag_reorder() {
        let tabs = sample_tabs();
        let m = model(&tabs, false);

        let mut ui = Ui::with_config(NousConfig::default());
        ui.set_size(1280.0, 800.0);
        assert_eq!(ui.mouse_down(&m, 60.0, 20.0), Some(UiAction::SelectTab(0)));
        let (redraw, mv) = ui.mouse_move(&m, 320.0, 20.0);
        assert!(redraw);
        assert_eq!(mv, Some(UiAction::MoveTab { from: 0, to: 1 }));
        ui.mouse_up(&m, 320.0, 20.0);

        let mut cfg = NousConfig::default();
        cfg.tab_position = TabPosition::Left;
        ui.apply_config(cfg);
        assert_eq!(ui.mouse_down(&m, 80.0, 95.0), Some(UiAction::SelectTab(0)));
        let (_, mv1) = ui.mouse_move(&m, 80.0, 129.0);
        assert_eq!(mv1, Some(UiAction::MoveTab { from: 0, to: 1 }));
        let (_, mv2) = ui.mouse_move(&m, 80.0, 165.0);
        assert_eq!(mv2, Some(UiAction::MoveTab { from: 1, to: 2 }));
    }

    #[test]
    fn sidebar_toggle_badges_menu_and_animating_invariant() {
        let tabs = sample_tabs();
        let m_loading = model(&tabs, true);

        let mut cfg = NousConfig::default();
        cfg.tab_position = TabPosition::Left;
        let mut ui = Ui::with_config(cfg);
        ui.set_size(1280.0, 800.0);
        assert!(ui.animating(&m_loading));
        assert_eq!(ui.content_rect().x0, 220.0);

        let geom = ui.layout(tabs.len());
        let btn_center = |hit: Hit| {
            geom.tool_buttons
                .iter()
                .find(|(h, _)| *h == hit)
                .unwrap()
                .1
                .center()
        };

        let sb_btn = btn_center(Hit::SidebarToggle);
        ui.mouse_down(&m_loading, sb_btn.x, sb_btn.y);
        assert_eq!(ui.mouse_up(&m_loading, sb_btn.x, sb_btn.y), Some(UiAction::ToggleSidebar));
        assert!(ui.config().sidebar_collapsed);
        assert_eq!(ui.content_rect().x0, 52.0);

        let perf_btn = btn_center(Hit::PerfBadge);
        ui.mouse_down(&m_loading, perf_btn.x, perf_btn.y);
        assert_eq!(ui.mouse_up(&m_loading, perf_btn.x, perf_btn.y), Some(UiAction::CyclePerfProfile));
        assert_eq!(ui.config().perf_profile, PerformanceProfile::MaxSpeed);
        ui.mouse_down(&m_loading, perf_btn.x, perf_btn.y);
        assert_eq!(ui.mouse_up(&m_loading, perf_btn.x, perf_btn.y), Some(UiAction::CyclePerfProfile));
        assert_eq!(ui.config().perf_profile, PerformanceProfile::UltraLight);
        assert!(!ui.animating(&m_loading));

        let ram_btn = btn_center(Hit::RamBadge);
        ui.mouse_down(&m_loading, ram_btn.x, ram_btn.y);
        assert_eq!(ui.mouse_up(&m_loading, ram_btn.x, ram_btn.y), Some(UiAction::OpenMemory));

        let home_btn = btn_center(Hit::Home);
        ui.mouse_down(&m_loading, home_btn.x, home_btn.y);
        assert_eq!(ui.mouse_up(&m_loading, home_btn.x, home_btn.y), Some(UiAction::Home));

        // Open menu and click ToggleZenMode
        let menu_btn = btn_center(Hit::Menu);
        ui.mouse_down(&m_loading, menu_btn.x, menu_btn.y);
        assert_eq!(ui.mouse_up(&m_loading, menu_btn.x, menu_btn.y), None);
        assert!(ui.menu_open());

        let items = ui.menu_items(&m_loading);
        let zen_idx = items.iter().position(|(_, c)| *c == MenuCommand::ToggleZenMode).unwrap();
        let mr = ui.menu_rect(&ui.layout(tabs.len()), items.len());
        let zen_y = mr.y0 + 4.0 + (zen_idx as f64 + 0.5) * MENU_ITEM_H;
        let zen_x = mr.center().x;
        ui.mouse_down(&m_loading, zen_x, zen_y);
        assert_eq!(
            ui.mouse_up(&m_loading, zen_x, zen_y),
            Some(UiAction::Menu(MenuCommand::ToggleZenMode))
        );
        assert_eq!(ui.config().tab_position, TabPosition::Hidden);
        assert!(!ui.menu_open());

        // Middle click closes tab
        ui.apply_config(NousConfig::default());
        assert_eq!(ui.middle_click(&m_loading, 50.0, 20.0), Some(UiAction::CloseTab(0)));
    }

    #[test]
    fn disabled_toolbar_modules_remove_hitboxes() {
        let tabs = sample_tabs();
        let m = model(&tabs, false);
        let mut cfg = NousConfig::default();
        cfg.toolbar_modules = ToolbarModules {
            back: false,
            forward: true,
            reload: true,
            home: false,
            omnibox: true,
            ram_badge: false,
            perf_badge: false,
            sidebar_toggle: false,
            menu: true,
        };
        let ui = Ui::with_config(cfg);
        assert_eq!(ui.hit_test(&m, 24.0, 62.0), Hit::Forward);
    }

    #[test]
    fn paint_all_presets_and_layouts_no_panic() {
        let tabs = sample_tabs();
        let m = model(&tabs, true);
        let mut le = LayoutEngine::new();
        let mut scene = Scene::new();

        for preset in ThemePreset::ALL {
            let mut cfg = NousConfig::default();
            cfg.theme_preset = preset;
            cfg.ui_font = "Segoe UI".into();
            cfg.ui_font_size = 13.5;
            cfg.ui_scale = 1.1;
            let mut ui = Ui::with_config(cfg);
            ui.set_telemetry(Some(48.7), 5);
            ui.focus_omnibox("https://nous.dev/docs");
            ui.menu_open = true;
            scene.reset();
            ui.paint(&mut scene, &mut le, &m, Affine::IDENTITY);
        }

        for tab_pos in TabPosition::ALL {
            for tb_pos in ToolbarPosition::ALL {
                for density in Density::ALL {
                    let mut cfg = NousConfig::default();
                    cfg.tab_position = tab_pos;
                    cfg.toolbar_position = tb_pos;
                    cfg.density = density;
                    cfg.sidebar_collapsed = tab_pos == TabPosition::Right;
                    let mut ui = Ui::with_config(cfg);
                    ui.set_size(1024.0, 720.0);
                    ui.set_telemetry(Some(64.0), 0);
                    scene.reset();
                    ui.paint(&mut scene, &mut le, &m, Affine::IDENTITY);
                }
            }
        }
    }

    #[test]
    fn menu_and_paint_narrow_and_zero_size_no_panic() {
        let tabs = sample_tabs();
        let m = model(&tabs, false);
        let mut le = LayoutEngine::new();
        let mut scene = Scene::new();

        for (w, h) in [(520.0, 320.0), (100.0, 80.0), (0.0, 0.0)] {
            for tb_pos in ToolbarPosition::ALL {
                for tab_pos in TabPosition::ALL {
                    let mut cfg = NousConfig::default();
                    cfg.tab_position = tab_pos;
                    cfg.toolbar_position = tb_pos;
                    cfg.ui_scale = 2.0;
                    let mut ui = Ui::with_config(cfg);
                    ui.set_size(w, h);
                    ui.menu_open = true;

                    let geom = ui.layout(tabs.len());
                    let mr = ui.menu_rect(&geom, ui.menu_items(&m).len());
                    assert!(mr.x0 <= mr.x1 && mr.y0 <= mr.y1);
                    let _ = ui.hit_test(&m, mr.center().x, mr.center().y);
                    let _ = ui.hit_test(&m, w * 0.5, h * 0.5);

                    scene.reset();
                    ui.paint(&mut scene, &mut le, &m, Affine::IDENTITY);
                }
            }
        }
    }
}
