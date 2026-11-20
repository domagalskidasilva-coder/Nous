//! Interface do Nous: barra de abas integrada à barra de título, barra de ferramentas,
//! omnibox e menu. Tudo desenhado com Vello, usando o mesmo sistema de fontes das páginas.
//!
//! A interface não sabe nada de janelas: o app traduz eventos do sistema para as chamadas
//! daqui e executa as [`UiAction`]s que voltam.

mod text_field;

use std::time::{Duration, Instant};

use nous_paint::{LayoutEngine, TextBrush, draw_text, to_color};
use parley::{FontFamily, FontFamilyName, FontWeight, GenericFamily, Layout, StyleProperty};
use vello::Scene;
use vello::kurbo::{
    Affine, Arc, BezPath, Circle, Line, Point, Rect, RoundedRect, RoundedRectRadii, Stroke, Vec2,
};
use vello::peniko::{Fill, Gradient};

pub use text_field::TextField;

pub const TABSTRIP_H: f64 = 40.0;
pub const TOOLBAR_H: f64 = 44.0;
/// Altura total da interface acima do conteúdo da página.
pub const CHROME_H: f64 = TABSTRIP_H + TOOLBAR_H;
const CAPTION_W: f64 = 46.0;
const TAB_MAX_W: f64 = 240.0;
const TAB_MIN_W: f64 = 56.0;
const MENU_W: f64 = 260.0;
const MENU_ITEM_H: f64 = 34.0;

type Rgba8 = [u8; 4];

/// Cores da interface. O escuro segue a identidade NOUS (fundo #07080D, violeta #7C5CFF,
/// ciano #22D3EE, texto #F5F6FB).
#[derive(Clone, Copy, Debug)]
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
    Omnibox,
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
    ToggleTheme,
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
    Navigate(String),
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
    moved: bool,
}

pub struct Ui {
    pub theme: Theme,
    pub omnibox: TextField,
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
        Ui {
            theme,
            omnibox: TextField::default(),
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

    /// Tamanho lógico da janela.
    pub fn set_size(&mut self, w: f64, h: f64) {
        self.size = (w, h);
    }

    pub fn content_rect(&self) -> Rect {
        Rect::new(0.0, CHROME_H, self.size.0, self.size.1.max(CHROME_H))
    }

    pub fn menu_open(&self) -> bool {
        self.menu_open
    }

    pub fn close_menu(&mut self) {
        self.menu_open = false;
    }

    /// A interface precisa de quadros contínuos (animação de carregamento)?
    pub fn animating(&self, m: &UiModel) -> bool {
        m.loading || m.tabs.iter().any(|t| t.loading)
    }

    pub fn focus_omnibox(&mut self, url: &str) {
        self.omnibox.focus_with(url);
        self.menu_open = false;
    }

    // ------------------------------------------------------------ geometria

    fn tab_metrics(&self, n: usize) -> (f64, f64) {
        let avail = self.size.0 - 8.0 - 44.0 - CAPTION_W * 3.0 - 12.0;
        let w = (avail / n.max(1) as f64).clamp(TAB_MIN_W, TAB_MAX_W);
        (8.0, w)
    }

    fn tab_rect(&self, n: usize, i: usize) -> Rect {
        let (x0, w) = self.tab_metrics(n);
        let x = x0 + i as f64 * w;
        Rect::new(x, 6.0, x + w, TABSTRIP_H)
    }

    fn tab_close_center(&self, n: usize, i: usize) -> Point {
        let r = self.tab_rect(n, i);
        Point::new(r.x1 - 18.0, (r.y0 + r.y1) / 2.0)
    }

    fn new_tab_rect(&self, n: usize) -> Rect {
        let (x0, w) = self.tab_metrics(n);
        let x = (x0 + n as f64 * w + 6.0).min(self.size.0 - CAPTION_W * 3.0 - 40.0);
        Rect::new(x, 8.0, x + 30.0, 38.0)
    }

    fn caption_rect(&self, k: usize) -> Rect {
        let x = self.size.0 - (3 - k) as f64 * CAPTION_W;
        Rect::new(x, 0.0, x + CAPTION_W, TABSTRIP_H - 6.0)
    }

    fn tool_rect(&self, k: usize) -> Rect {
        let x = 8.0 + k as f64 * 36.0;
        let y = TABSTRIP_H + 6.0;
        Rect::new(x, y, x + 32.0, y + 32.0)
    }

    fn omnibox_rect(&self) -> Rect {
        Rect::new(
            124.0,
            TABSTRIP_H + 5.0,
            (self.size.0 - 52.0).max(200.0),
            TABSTRIP_H + 39.0,
        )
    }

    fn menu_button_rect(&self) -> Rect {
        let x = self.size.0 - 44.0;
        Rect::new(x, TABSTRIP_H + 6.0, x + 32.0, TABSTRIP_H + 38.0)
    }

    fn menu_items(&self, m: &UiModel) -> Vec<(String, MenuCommand)> {
        vec![
            (
                "Nova aba                       Ctrl+T".into(),
                MenuCommand::NewTab,
            ),
            (
                format!("Buscador: {}", m.search_name),
                MenuCommand::NextSearchEngine,
            ),
            (
                if self.theme.dark {
                    "Tema claro"
                } else {
                    "Tema escuro"
                }
                .into(),
                MenuCommand::ToggleTheme,
            ),
            ("Memória (nous://memoria)".into(), MenuCommand::Memory),
            ("Sobre o Nous".into(), MenuCommand::About),
        ]
    }

    fn menu_rect(&self, count: usize) -> Rect {
        let x1 = self.size.0 - 12.0;
        let y0 = CHROME_H - 4.0;
        Rect::new(x1 - MENU_W, y0, x1, y0 + 8.0 + count as f64 * MENU_ITEM_H)
    }

    pub fn hit_test(&self, m: &UiModel, x: f64, y: f64) -> Hit {
        let p = Point::new(x, y);
        if self.menu_open {
            let items = self.menu_items(m).len();
            let r = self.menu_rect(items);
            if r.contains(p) {
                let i = ((y - r.y0 - 4.0) / MENU_ITEM_H).floor().max(0.0) as usize;
                return Hit::MenuItem(i.min(items - 1));
            }
        }
        for (k, hit) in [Hit::Minimize, Hit::Maximize, Hit::Close]
            .into_iter()
            .enumerate()
        {
            if self.caption_rect(k).contains(p) {
                return hit;
            }
        }
        let n = m.tabs.len();
        if y < TABSTRIP_H {
            for i in 0..n {
                let r = self.tab_rect(n, i);
                if r.contains(p) {
                    let c = self.tab_close_center(n, i);
                    let show_close = r.width() > 90.0 || i == m.active;
                    if show_close && (p - c).hypot() <= 9.0 {
                        return Hit::TabClose(i);
                    }
                    return Hit::Tab(i);
                }
            }
            if self.new_tab_rect(n).contains(p) {
                return Hit::NewTab;
            }
            return Hit::DragArea;
        }
        if y < CHROME_H {
            for (k, hit) in [Hit::Back, Hit::Forward, Hit::Reload]
                .into_iter()
                .enumerate()
            {
                if self.tool_rect(k).contains(p) {
                    return hit;
                }
            }
            if self.omnibox_rect().contains(p) {
                return Hit::Omnibox;
            }
            if self.menu_button_rect().contains(p) {
                return Hit::Menu;
            }
            return Hit::None;
        }
        Hit::Content
    }

    // ------------------------------------------------------------- eventos

    /// Movimento do mouse. Devolve (precisa redesenhar, ação).
    pub fn mouse_move(&mut self, m: &UiModel, x: f64, y: f64) -> (bool, Option<UiAction>) {
        let hit = self.hit_test(m, x, y);
        let mut redraw = hit != self.hover;
        self.hover = hit;
        let mut action = None;
        let n = m.tabs.len();
        if let Some(drag) = &mut self.drag {
            if !drag.moved && (x - drag.start_x).abs() > 5.0 {
                drag.moved = true;
            }
            if drag.moved {
                let (x0, w) = {
                    let avail = self.size.0 - 8.0 - 44.0 - CAPTION_W * 3.0 - 12.0;
                    (8.0, (avail / n.max(1) as f64).clamp(TAB_MIN_W, TAB_MAX_W))
                };
                let target = (((x - x0) / w).floor().max(0.0) as usize).min(n.saturating_sub(1));
                if target != drag.index {
                    action = Some(UiAction::MoveTab {
                        from: drag.index,
                        to: target,
                    });
                    drag.index = target;
                    drag.start_x = x;
                    redraw = true;
                }
            }
        }
        (redraw, action)
    }

    pub fn mouse_leave(&mut self) -> bool {
        let changed = self.hover != Hit::None;
        self.hover = Hit::None;
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
                    moved: false,
                });
                Some(UiAction::SelectTab(i))
            }
            Hit::DragArea if double => Some(UiAction::ToggleMaximize),
            Hit::DragArea => Some(UiAction::DragWindow),
            Hit::Omnibox => {
                if !self.omnibox.focused {
                    self.omnibox.focus_with(m.url);
                } else {
                    let local = x - self.omnibox_rect().x0 - 16.0;
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
            Hit::Reload => Some(if m.loading {
                UiAction::Stop
            } else {
                UiAction::Reload
            }),
            Hit::Minimize => Some(UiAction::Minimize),
            Hit::Maximize => Some(UiAction::ToggleMaximize),
            Hit::Close => Some(UiAction::CloseWindow),
            Hit::Menu => {
                self.menu_open = !self.menu_open;
                None
            }
            Hit::MenuItem(i) => {
                self.menu_open = false;
                self.menu_items(m).get(i).map(|(_, c)| UiAction::Menu(*c))
            }
            _ => None,
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
        b.push_default(StyleProperty::FontFamily(FontFamily::Single(
            FontFamilyName::Generic(GenericFamily::SystemUi),
        )));
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
        let th = self.theme;
        let (w, _) = self.size;
        let time = self.started.elapsed().as_secs_f64();

        // faixa das abas
        Self::fill(scene, t, th.frame, &Rect::new(0.0, 0.0, w, TABSTRIP_H));
        let n = m.tabs.len();
        for (i, tab) in m.tabs.iter().enumerate() {
            let r = self.tab_rect(n, i);
            let active = i == m.active;
            let hovered = matches!(self.hover, Hit::Tab(j) | Hit::TabClose(j) if j == i);
            if active {
                let shape = RoundedRect::from_rect(r, RoundedRectRadii::new(9.0, 9.0, 0.0, 0.0));
                Self::fill(scene, t, th.surface, &shape);
            } else if hovered {
                let shape = RoundedRect::from_rect(
                    Rect::new(r.x0 + 2.0, r.y0 + 1.0, r.x1 - 2.0, r.y1 - 5.0),
                    8.0,
                );
                Self::fill(scene, t, th.hover, &shape);
            } else if i + 1 < n
                && i + 1 != m.active
                && !matches!(self.hover, Hit::Tab(j) | Hit::TabClose(j) if j == i + 1)
            {
                Self::stroke(
                    scene,
                    t,
                    th.separator,
                    1.0,
                    &Line::new((r.x1, 14.0), (r.x1, 32.0)),
                );
            }
            let cy = (r.y0 + r.y1) / 2.0;
            let icon = Point::new(r.x0 + 18.0, cy);
            if tab.loading {
                let start = time * 6.0;
                Self::stroke(
                    scene,
                    t,
                    th.accent,
                    2.0,
                    &Arc::new(icon, (6.0, 6.0), start, 4.2, 0.0),
                );
            } else {
                let grad = Gradient::new_linear(
                    (icon.x - 6.0, icon.y - 6.0),
                    (icon.x + 6.0, icon.y + 6.0),
                )
                .with_stops([to_color(th.accent), to_color(th.accent2)]);
                scene.fill(Fill::NonZero, t, &grad, None, &Circle::new(icon, 6.0));
            }
            let show_close = r.width() > 90.0 || active;
            let text_x1 = if show_close { r.x1 - 30.0 } else { r.x1 - 8.0 };
            let color = if active { th.text } else { th.text_dim };
            let layout = Self::text(
                le,
                tab.title,
                12.5,
                if active { 600.0 } else { 500.0 },
                color,
            );
            let clip = Rect::new(r.x0 + 30.0, r.y0, text_x1, r.y1);
            if clip.width() > 4.0 {
                scene.push_clip_layer(Fill::NonZero, t, &clip);
                let ty = cy - layout.height() as f64 / 2.0;
                draw_text(
                    scene,
                    &layout,
                    t * Affine::translate((r.x0 + 30.0, ty)),
                    None,
                );
                scene.pop_layer();
            }
            if show_close {
                let c = self.tab_close_center(n, i);
                if self.hover == Hit::TabClose(i) {
                    Self::fill(scene, t, th.hover, &Circle::new(c, 9.0));
                }
                let d = 3.5;
                Self::stroke(
                    scene,
                    t,
                    th.text_dim,
                    1.4,
                    &Line::new((c.x - d, c.y - d), (c.x + d, c.y + d)),
                );
                Self::stroke(
                    scene,
                    t,
                    th.text_dim,
                    1.4,
                    &Line::new((c.x - d, c.y + d), (c.x + d, c.y - d)),
                );
            }
        }
        // botão de nova aba
        let nt = self.new_tab_rect(n);
        let c = nt.center();
        if self.hover == Hit::NewTab {
            Self::fill(scene, t, th.hover, &Circle::new(c, 14.0));
        }
        Self::stroke(
            scene,
            t,
            th.text,
            1.6,
            &Line::new((c.x - 6.0, c.y), (c.x + 6.0, c.y)),
        );
        Self::stroke(
            scene,
            t,
            th.text,
            1.6,
            &Line::new((c.x, c.y - 6.0), (c.x, c.y + 6.0)),
        );

        // botões da janela (minimizar, maximizar, fechar)
        for (k, hit) in [Hit::Minimize, Hit::Maximize, Hit::Close]
            .into_iter()
            .enumerate()
        {
            let r = self.caption_rect(k);
            let hovered = self.hover == hit;
            if hovered {
                Self::fill(
                    scene,
                    t,
                    if hit == Hit::Close {
                        th.danger
                    } else {
                        th.hover
                    },
                    &r,
                );
            }
            let color = if hovered && hit == Hit::Close {
                [255, 255, 255, 255]
            } else {
                th.text
            };
            let c = r.center();
            match hit {
                Hit::Minimize => Self::stroke(
                    scene,
                    t,
                    color,
                    1.0,
                    &Line::new((c.x - 5.0, c.y), (c.x + 5.0, c.y)),
                ),
                Hit::Maximize if m.maximized => {
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.0,
                        &Rect::new(c.x - 5.0, c.y - 3.0, c.x + 3.0, c.y + 5.0),
                    );
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.0,
                        &Line::new((c.x - 3.0, c.y - 5.0), (c.x + 5.0, c.y - 5.0)),
                    );
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.0,
                        &Line::new((c.x + 5.0, c.y - 5.0), (c.x + 5.0, c.y + 3.0)),
                    );
                }
                Hit::Maximize => Self::stroke(
                    scene,
                    t,
                    color,
                    1.0,
                    &Rect::new(c.x - 5.0, c.y - 5.0, c.x + 5.0, c.y + 5.0),
                ),
                _ => {
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.0,
                        &Line::new((c.x - 5.0, c.y - 5.0), (c.x + 5.0, c.y + 5.0)),
                    );
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.0,
                        &Line::new((c.x - 5.0, c.y + 5.0), (c.x + 5.0, c.y - 5.0)),
                    );
                }
            }
        }

        // barra de ferramentas
        Self::fill(
            scene,
            t,
            th.surface,
            &Rect::new(0.0, TABSTRIP_H, w, CHROME_H),
        );
        Self::stroke(
            scene,
            t,
            th.separator,
            1.0,
            &Line::new((0.0, CHROME_H - 0.5), (w, CHROME_H - 0.5)),
        );
        for (k, hit) in [Hit::Back, Hit::Forward, Hit::Reload]
            .into_iter()
            .enumerate()
        {
            let r = self.tool_rect(k);
            let c = r.center();
            let enabled = match hit {
                Hit::Back => m.can_back,
                Hit::Forward => m.can_forward,
                _ => true,
            };
            if enabled && self.hover == hit {
                Self::fill(scene, t, th.hover, &Circle::new(c, 15.0));
            }
            let color = if enabled {
                th.text
            } else {
                Self::with_alpha(th.text_dim, 110)
            };
            match hit {
                Hit::Back | Hit::Forward => {
                    let s = if hit == Hit::Back { 1.0 } else { -1.0 };
                    let mut p = BezPath::new();
                    p.move_to((c.x + 6.0 * s, c.y));
                    p.line_to((c.x - 6.0 * s, c.y));
                    p.move_to((c.x - 1.0 * s, c.y - 5.0));
                    p.line_to((c.x - 6.0 * s, c.y));
                    p.line_to((c.x - 1.0 * s, c.y + 5.0));
                    Self::stroke(scene, t, color, 1.7, &p);
                }
                _ if m.loading => {
                    let d = 5.0;
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.7,
                        &Line::new((c.x - d, c.y - d), (c.x + d, c.y + d)),
                    );
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.7,
                        &Line::new((c.x - d, c.y + d), (c.x + d, c.y - d)),
                    );
                }
                _ => {
                    Self::stroke(
                        scene,
                        t,
                        color,
                        1.7,
                        &Arc::new(c, (6.0, 6.0), -1.2, 5.0, 0.0),
                    );
                    let tip = Point::new(c.x + 6.0 * (-1.2f64).cos(), c.y + 6.0 * (-1.2f64).sin());
                    let mut p = BezPath::new();
                    p.move_to((tip.x - 4.0, tip.y - 2.0));
                    p.line_to(tip);
                    p.line_to((tip.x + 0.5, tip.y + 4.5));
                    Self::stroke(scene, t, color, 1.7, &p);
                }
            }
        }

        // omnibox
        let ob = self.omnibox_rect();
        let focused = self.omnibox.focused;
        let field = if focused { th.surface } else { th.field };
        let shape = RoundedRect::from_rect(ob, ob.height() / 2.0);
        Self::fill(scene, t, field, &shape);
        if focused {
            Self::stroke(scene, t, th.accent, 2.0, &shape);
        } else if self.hover == Hit::Omnibox {
            Self::stroke(scene, t, th.separator, 1.0, &shape);
        }
        let tx = ob.x0 + 16.0;
        let inner = Rect::new(ob.x0 + 10.0, ob.y0, ob.x1 - 12.0, ob.y1);
        scene.push_clip_layer(Fill::NonZero, t, &inner);
        if focused {
            if let Some(local) = self.pending_click_x.take() {
                let idx = self.byte_index_at(le, local);
                self.omnibox.set_cursor(idx, false);
            }
            let f = &self.omnibox;
            let x_of = |le: &mut LayoutEngine, idx: usize| -> f64 {
                if idx == 0 {
                    0.0
                } else {
                    Self::text(le, &f.text[..idx], 14.0, 400.0, th.text).full_width() as f64
                }
            };
            if let Some((a, b)) = f.selection() {
                let (xa, xb) = (x_of(le, a), x_of(le, b));
                let sel = Rect::new(tx + xa, ob.y0 + 8.0, tx + xb, ob.y1 - 8.0);
                Self::fill(scene, t, Self::with_alpha(th.accent, 90), &sel);
            }
            let layout = Self::text(le, &f.text, 14.0, 400.0, th.text);
            let ty = ob.center().y - layout.height() as f64 / 2.0;
            draw_text(scene, &layout, t * Affine::translate((tx, ty)), None);
            let cx = tx + x_of(le, f.cursor);
            Self::stroke(
                scene,
                t,
                th.text,
                1.2,
                &Line::new((cx, ob.y0 + 9.0), (cx, ob.y1 - 9.0)),
            );
        } else {
            let (text, color) = if m.url.is_empty() {
                (
                    format!("Pesquise com {} ou digite um endereço", m.search_name),
                    th.text_dim,
                )
            } else {
                (m.url.to_owned(), th.text)
            };
            let layout = Self::text(le, &text, 14.0, 400.0, color);
            let ty = ob.center().y - layout.height() as f64 / 2.0;
            draw_text(scene, &layout, t * Affine::translate((tx, ty)), None);
        }
        scene.pop_layer();

        // menu ⋮
        let mb = self.menu_button_rect();
        let c = mb.center();
        if self.hover == Hit::Menu || self.menu_open {
            Self::fill(scene, t, th.hover, &Circle::new(c, 15.0));
        }
        for dy in [-5.0, 0.0, 5.0] {
            Self::fill(scene, t, th.text, &Circle::new((c.x, c.y + dy), 1.6));
        }

        // barra de progresso (violeta → ciano) enquanto carrega
        if m.loading {
            let phase = (time * 0.7).fract();
            let seg = w * 0.35;
            let x0 = -seg + phase * (w + seg);
            let bar = Rect::new(
                x0.max(0.0),
                CHROME_H - 2.5,
                (x0 + seg).min(w),
                CHROME_H - 0.5,
            );
            let grad = Gradient::new_linear((x0, 0.0), (x0 + seg, 0.0))
                .with_stops([to_color(th.accent), to_color(th.accent2)]);
            scene.fill(Fill::NonZero, t, &grad, None, &bar);
        }

        // menu aberto
        if self.menu_open {
            let items = self.menu_items(m);
            let r = self.menu_rect(items.len());
            let shadow = RoundedRect::from_rect(r.inflate(2.0, 2.0) + Vec2::new(0.0, 3.0), 12.0);
            Self::fill(scene, t, [0, 0, 0, if th.dark { 120 } else { 40 }], &shadow);
            let panel = RoundedRect::from_rect(r, 10.0);
            Self::fill(scene, t, th.surface, &panel);
            Self::stroke(scene, t, th.separator, 1.0, &panel);
            for (i, (label, _)) in items.iter().enumerate() {
                let y0 = r.y0 + 4.0 + i as f64 * MENU_ITEM_H;
                let row = Rect::new(r.x0 + 4.0, y0, r.x1 - 4.0, y0 + MENU_ITEM_H);
                if self.hover == Hit::MenuItem(i) {
                    Self::fill(scene, t, th.hover, &RoundedRect::from_rect(row, 6.0));
                }
                let layout = Self::text(le, label, 13.0, 400.0, th.text);
                let ty = row.center().y - layout.height() as f64 / 2.0;
                draw_text(
                    scene,
                    &layout,
                    t * Affine::translate((row.x0 + 12.0, ty)),
                    None,
                );
            }
        }
    }

    /// Índice (em bytes) do caractere mais próximo de `x` dentro do texto da omnibox.
    fn byte_index_at(&self, le: &mut LayoutEngine, x: f64) -> usize {
        let text = &self.omnibox.text;
        let mut best = (0usize, f64::MAX);
        let mut boundaries: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        boundaries.push(text.len());
        for idx in boundaries {
            let w = if idx == 0 {
                0.0
            } else {
                Self::text(le, &text[..idx], 14.0, 400.0, self.theme.text).full_width() as f64
            };
            let d = (w - x).abs();
            if d < best.1 {
                best = (idx, d);
            }
        }
        best.0
    }
}
