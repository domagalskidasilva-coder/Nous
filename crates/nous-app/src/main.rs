//! Nous — navegador leve feito em Rust.
//!
//! Este binário junta as peças: janela (winit), GPU (wgpu via Vello), interface (nous-ui)
//! e motor (nous-engine). Só redesenha quando algo muda (ou enquanto uma página carrega),
//! então parado ele não gasta CPU.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod capture;
mod pages;

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use nous_engine::{
    Engine, EngineEvent, PageViewport, SearchEngine, Url, ViewId, WebView, resolve_input,
};
use nous_paint::{LayoutEngine, rgba, to_color};
use nous_ui::{CHROME_H, Hit, MenuCommand, TabView, Theme, Ui, UiAction, UiKey, UiModel};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Color, Fill};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, ResizeDirection, Window, WindowId};

const NEW_TAB_URL: &str = "nous://nova-aba";

struct Tab {
    view: WebView,
}

struct Gpu {
    cx: RenderContext,
    surface: RenderSurface<'static>,
    renderer: Renderer,
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    scene: Scene,
    le: LayoutEngine,
    ui: Ui,
    engine: Engine,
    tabs: Vec<Tab>,
    active: usize,
    next_view_id: ViewId,
    search: SearchEngine,
    theme_override: Option<bool>,
    modifiers: ModifiersState,
    cursor: (f64, f64),
    cursor_icon: CursorIcon,
    content_press: Option<(f64, f64)>,
    clipboard: Option<arboard::Clipboard>,
}

impl App {
    fn new(proxy: EventLoopProxy<EngineEvent>) -> Result<App, String> {
        let proxy = Mutex::new(proxy);
        let engine = Engine::new(move |ev| {
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(ev);
            }
        })?;
        let mut app = App {
            window: None,
            gpu: None,
            scene: Scene::new(),
            le: LayoutEngine::new(),
            ui: Ui::new(Theme::dark()),
            engine,
            tabs: Vec::new(),
            active: 0,
            next_view_id: 1,
            search: SearchEngine::DuckDuckGo,
            theme_override: None,
            modifiers: ModifiersState::empty(),
            cursor: (0.0, 0.0),
            cursor_icon: CursorIcon::Default,
            content_press: None,
            clipboard: None,
        };
        let initial = std::env::args().nth(1);
        app.new_tab(initial.as_deref());
        Ok(app)
    }

    // ------------------------------------------------------------- abas

    fn new_tab(&mut self, input: Option<&str>) {
        let id = self.next_view_id;
        self.next_view_id += 1;
        let mut tab = Tab {
            view: WebView::new(id),
        };
        if let Some(prev) = self.tabs.get_mut(self.active) {
            prev.view.discard_layout();
        }
        match input {
            Some(text) => {
                let url = resolve_input(text, self.search);
                self.open_url(&mut tab.view, url, true);
                self.ui.omnibox.blur();
            }
            None => {
                let url = Url::parse(NEW_TAB_URL).expect("url interna");
                tab.view.show_internal(url, &pages::new_tab(), true);
                self.ui.focus_omnibox("");
            }
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.update_title();
    }

    fn select_tab(&mut self, i: usize) {
        if i >= self.tabs.len() || i == self.active {
            return;
        }
        // aba que sai de cena libera estilo e layout (recalculados ao voltar)
        self.tabs[self.active].view.discard_layout();
        self.active = i;
        self.ui.omnibox.blur();
        self.update_title();
    }

    fn close_tab(&mut self, i: usize, event_loop: &ActiveEventLoop) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            event_loop.exit();
            return;
        }
        if self.active > i || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.update_title();
    }

    fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        if self.active == from {
            self.active = to;
        } else if from < self.active && to >= self.active {
            self.active -= 1;
        } else if from > self.active && to <= self.active {
            self.active += 1;
        }
    }

    /// Abre `url` numa WebView; páginas `nous://` são geradas aqui mesmo.
    fn open_url(&mut self, view: &mut WebView, url: Url, new_entry: bool) {
        if url.scheme() == "nous" {
            let html = self.internal_page(&url);
            view.show_internal(url, &html, new_entry);
        } else {
            view.navigate(&self.engine, url);
        }
    }

    fn navigate_active(&mut self, url: Url) {
        let mut view = std::mem::replace(&mut self.tabs[self.active].view, WebView::new(0));
        self.open_url(&mut view, url, true);
        self.tabs[self.active].view = view;
        self.update_title();
    }

    fn internal_page(&self, url: &Url) -> String {
        match url.host_str().unwrap_or("") {
            "memoria" => {
                let rows: Vec<pages::TabMemory> = self
                    .tabs
                    .iter()
                    .map(|t| pages::TabMemory {
                        title: t.view.title(),
                        report: t.view.memory(),
                    })
                    .collect();
                pages::memory(&rows, nous_engine::process_memory_bytes())
            }
            "sobre" => pages::about(),
            _ => pages::new_tab(),
        }
    }

    fn update_title(&self) {
        if let (Some(w), Some(tab)) = (&self.window, self.tabs.get(self.active)) {
            w.set_title(&format!("{} — Nous", tab.view.title()));
        }
    }

    fn redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn scale(&self) -> f64 {
        self.window
            .as_ref()
            .map(|w| w.scale_factor())
            .unwrap_or(1.0)
    }

    fn logical_size(&self) -> (f64, f64) {
        match &self.window {
            Some(w) => {
                let s = w.inner_size();
                let k = w.scale_factor();
                (s.width as f64 / k, s.height as f64 / k)
            }
            None => (1280.0, 800.0),
        }
    }

    fn viewport(&self) -> PageViewport {
        let (w, h) = self.logical_size();
        PageViewport {
            width: w as f32,
            height: (h - CHROME_H).max(0.0) as f32,
        }
    }

    // ------------------------------------------------------- modelo da UI

    fn with_model<R>(
        &mut self,
        f: impl FnOnce(&mut Ui, &UiModel, &mut LayoutEngine, &mut Scene) -> R,
    ) -> R {
        let titles: Vec<String> = self.tabs.iter().map(|t| t.view.title()).collect();
        let tabs: Vec<TabView> = self
            .tabs
            .iter()
            .zip(&titles)
            .map(|(t, title)| TabView {
                title,
                loading: t.view.is_loading(),
            })
            .collect();
        let view = &self.tabs[self.active].view;
        let url = match view.url() {
            Some(u) if u.as_str() == NEW_TAB_URL => String::new(),
            Some(u) => u.to_string(),
            None => String::new(),
        };
        let maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
        let model = UiModel {
            tabs: &tabs,
            active: self.active,
            url: &url,
            can_back: view.can_go_back(),
            can_forward: view.can_go_forward(),
            loading: view.is_loading(),
            maximized,
            search_name: self.search.name(),
        };
        f(&mut self.ui, &model, &mut self.le, &mut self.scene)
    }

    fn apply(&mut self, action: UiAction, event_loop: &ActiveEventLoop) {
        match action {
            UiAction::SelectTab(i) => self.select_tab(i),
            UiAction::CloseTab(i) => self.close_tab(i, event_loop),
            UiAction::NewTab | UiAction::Menu(MenuCommand::NewTab) => self.new_tab(None),
            UiAction::MoveTab { from, to } => self.move_tab(from, to),
            UiAction::Back => {
                let mut view = std::mem::replace(&mut self.tabs[self.active].view, WebView::new(0));
                if let Some(url) = view.go_back(&self.engine) {
                    let html = self.internal_page(&url);
                    view.show_internal(url, &html, false);
                }
                self.tabs[self.active].view = view;
            }
            UiAction::Forward => {
                let mut view = std::mem::replace(&mut self.tabs[self.active].view, WebView::new(0));
                if let Some(url) = view.go_forward(&self.engine) {
                    let html = self.internal_page(&url);
                    view.show_internal(url, &html, false);
                }
                self.tabs[self.active].view = view;
            }
            UiAction::Reload => {
                let mut view = std::mem::replace(&mut self.tabs[self.active].view, WebView::new(0));
                if let Some(url) = view.reload(&self.engine) {
                    let html = self.internal_page(&url);
                    view.show_internal(url, &html, false);
                }
                self.tabs[self.active].view = view;
            }
            UiAction::Stop => self.tabs[self.active].view.stop(),
            UiAction::Navigate(text) => {
                let url = resolve_input(&text, self.search);
                self.navigate_active(url);
            }
            UiAction::Minimize => {
                if let Some(w) = &self.window {
                    w.set_minimized(true);
                }
            }
            UiAction::ToggleMaximize => {
                if let Some(w) = &self.window {
                    w.set_maximized(!w.is_maximized());
                }
            }
            UiAction::CloseWindow => event_loop.exit(),
            UiAction::DragWindow => {
                if let Some(w) = &self.window {
                    let _ = w.drag_window();
                }
            }
            UiAction::Copy(text) => {
                if let Some(cb) = self.clipboard() {
                    let _ = cb.set_text(text);
                }
            }
            UiAction::Menu(MenuCommand::Memory) => {
                self.new_tab(Some("nous://memoria"));
            }
            UiAction::Menu(MenuCommand::About) => self.new_tab(Some("nous://sobre")),
            UiAction::Menu(MenuCommand::ToggleTheme) => {
                let dark = !self.ui.theme.dark;
                self.theme_override = Some(dark);
                self.ui.theme = if dark { Theme::dark() } else { Theme::light() };
            }
            UiAction::Menu(MenuCommand::NextSearchEngine) => {
                let i = SearchEngine::ALL
                    .iter()
                    .position(|s| *s == self.search)
                    .unwrap_or(0);
                self.search = SearchEngine::ALL[(i + 1) % SearchEngine::ALL.len()];
            }
        }
        self.update_title();
        self.redraw();
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    // ------------------------------------------------------------- teclado

    fn on_key(&mut self, event: KeyEvent, event_loop: &ActiveEventLoop) {
        if event.state != ElementState::Pressed {
            return;
        }
        let m = self.modifiers;
        let ctrl = if cfg!(target_os = "macos") {
            m.super_key()
        } else {
            m.control_key()
        };
        let shift = m.shift_key();
        let alt = m.alt_key();

        // atalhos globais
        let shortcut = match &event.logical_key {
            Key::Character(c) if ctrl => match c.to_lowercase().as_str() {
                "t" => Some(UiAction::NewTab),
                "w" => Some(UiAction::CloseTab(self.active)),
                "r" => Some(UiAction::Reload),
                "l" => {
                    let url = self.with_model(|_, m, _, _| m.url.to_owned());
                    self.ui.focus_omnibox(&url);
                    self.redraw();
                    return;
                }
                _ => None,
            },
            Key::Named(NamedKey::Tab) if ctrl => {
                let n = self.tabs.len();
                let i = if shift {
                    (self.active + n - 1) % n
                } else {
                    (self.active + 1) % n
                };
                Some(UiAction::SelectTab(i))
            }
            Key::Named(NamedKey::F5) => Some(UiAction::Reload),
            Key::Named(NamedKey::F6) => {
                let url = self.with_model(|_, m, _, _| m.url.to_owned());
                self.ui.focus_omnibox(&url);
                self.redraw();
                return;
            }
            Key::Named(NamedKey::ArrowLeft) if alt => Some(UiAction::Back),
            Key::Named(NamedKey::ArrowRight) if alt => Some(UiAction::Forward),
            _ => None,
        };
        if let Some(action) = shortcut {
            self.apply(action, event_loop);
            return;
        }

        // omnibox
        if self.ui.omnibox.focused {
            let key = match &event.logical_key {
                Key::Named(NamedKey::Backspace) => Some(UiKey::Backspace),
                Key::Named(NamedKey::Delete) => Some(UiKey::Delete),
                Key::Named(NamedKey::ArrowLeft) => Some(UiKey::Left { shift, word: ctrl }),
                Key::Named(NamedKey::ArrowRight) => Some(UiKey::Right { shift, word: ctrl }),
                Key::Named(NamedKey::Home) => Some(UiKey::Home { shift }),
                Key::Named(NamedKey::End) => Some(UiKey::End { shift }),
                Key::Named(NamedKey::Enter) => Some(UiKey::Enter),
                Key::Named(NamedKey::Escape) => Some(UiKey::Escape),
                Key::Character(c) if ctrl => match c.to_lowercase().as_str() {
                    "a" => Some(UiKey::SelectAll),
                    "c" => Some(UiKey::Copy),
                    "x" => Some(UiKey::Cut),
                    "v" => self
                        .clipboard()
                        .and_then(|cb| cb.get_text().ok())
                        .map(UiKey::Paste),
                    _ => None,
                },
                _ => event
                    .text
                    .as_ref()
                    .filter(|t| !ctrl && !t.chars().any(char::is_control))
                    .map(|t| UiKey::Text(t.to_string())),
            };
            if let Some(key) = key
                && let Some(action) = self.ui.key(key)
            {
                if let Some(action) = action {
                    self.apply(action, event_loop);
                }
                self.redraw();
            }
            return;
        }

        // rolagem da página
        let vp = self.viewport();
        let view = &mut self.tabs[self.active].view;
        let page = vp.height * 0.9;
        let dy = match &event.logical_key {
            Key::Named(NamedKey::ArrowDown) => Some(48.0),
            Key::Named(NamedKey::ArrowUp) => Some(-48.0),
            Key::Named(NamedKey::PageDown) => Some(page),
            Key::Named(NamedKey::PageUp) => Some(-page),
            Key::Named(NamedKey::Space) => Some(if shift { -page } else { page }),
            Key::Named(NamedKey::Home) => {
                view.scroll_to(0.0, vp.height);
                Some(0.0)
            }
            Key::Named(NamedKey::End) => {
                view.scroll_to(f32::MAX, vp.height);
                Some(0.0)
            }
            _ => None,
        };
        if let Some(dy) = dy {
            view.scroll_by(dy, vp.height);
            self.redraw();
        }
    }

    // ---------------------------------------------------------------- mouse

    fn resize_edge(&self, x: f64, y: f64) -> Option<ResizeDirection> {
        let w = self.window.as_ref()?;
        if w.is_maximized() {
            return None;
        }
        let (lw, lh) = self.logical_size();
        let e = 5.0;
        let (l, r, t, b) = (x < e, x > lw - e, y < e, y > lh - e);
        Some(match (l, r, t, b) {
            (true, _, true, _) => ResizeDirection::NorthWest,
            (_, true, true, _) => ResizeDirection::NorthEast,
            (true, _, _, true) => ResizeDirection::SouthWest,
            (_, true, _, true) => ResizeDirection::SouthEast,
            (true, ..) => ResizeDirection::West,
            (_, true, ..) => ResizeDirection::East,
            (_, _, true, _) => ResizeDirection::North,
            (.., true) => ResizeDirection::South,
            _ => return None,
        })
    }

    fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.window {
                w.set_cursor(icon);
            }
        }
    }

    fn on_cursor_moved(&mut self, x: f64, y: f64, event_loop: &ActiveEventLoop) {
        self.cursor = (x, y);
        let (redraw, action) = self.with_model(|ui, m, _, _| ui.mouse_move(m, x, y));
        if let Some(a) = action {
            self.apply(a, event_loop);
        }
        let icon = if let Some(dir) = self.resize_edge(x, y) {
            match dir {
                ResizeDirection::East | ResizeDirection::West => CursorIcon::EwResize,
                ResizeDirection::North | ResizeDirection::South => CursorIcon::NsResize,
                ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::NeswResize,
                _ => CursorIcon::NwseResize,
            }
        } else if y >= CHROME_H && !self.ui.menu_open() {
            let link = self.tabs[self.active]
                .view
                .link_at(x as f32, (y - CHROME_H) as f32);
            if link.is_some() {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            }
        } else {
            let hit = self.with_model(|ui, m, _, _| ui.hit_test(m, x, y));
            if hit == Hit::Omnibox {
                CursorIcon::Text
            } else {
                CursorIcon::Default
            }
        };
        self.set_cursor_icon(icon);
        if redraw {
            self.redraw();
        }
    }

    fn on_mouse(&mut self, state: ElementState, button: MouseButton, event_loop: &ActiveEventLoop) {
        let (x, y) = self.cursor;
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                if let Some(dir) = self.resize_edge(x, y)
                    && let Some(w) = &self.window
                {
                    let _ = w.drag_resize_window(dir);
                    return;
                }
                let action = self.with_model(|ui, m, _, _| ui.mouse_down(m, x, y));
                if y >= CHROME_H && !self.ui.menu_open() {
                    self.content_press = Some((x, y));
                }
                if let Some(a) = action {
                    self.apply(a, event_loop);
                }
                self.redraw();
            }
            (MouseButton::Left, ElementState::Released) => {
                let action = self.with_model(|ui, m, _, _| ui.mouse_up(m, x, y));
                if let Some(a) = action {
                    self.apply(a, event_loop);
                } else if let Some((px, py)) = self.content_press.take()
                    && (px - x).abs() < 6.0
                    && (py - y).abs() < 6.0
                    && let Some(url) = self.tabs[self.active]
                        .view
                        .link_at(x as f32, (y - CHROME_H) as f32)
                {
                    let ctrl = self.modifiers.control_key() || self.modifiers.super_key();
                    if ctrl {
                        self.open_background_tab(url);
                    } else {
                        self.navigate_active(url);
                    }
                }
                self.content_press = None;
                self.redraw();
            }
            (MouseButton::Middle, ElementState::Pressed) => {
                if y >= CHROME_H {
                    if let Some(url) = self.tabs[self.active]
                        .view
                        .link_at(x as f32, (y - CHROME_H) as f32)
                    {
                        self.open_background_tab(url);
                    }
                } else if let Some(a) = self.with_model(|ui, m, _, _| ui.middle_click(m, x, y)) {
                    self.apply(a, event_loop);
                }
                self.redraw();
            }
            (MouseButton::Back, ElementState::Pressed) => self.apply(UiAction::Back, event_loop),
            (MouseButton::Forward, ElementState::Pressed) => {
                self.apply(UiAction::Forward, event_loop)
            }
            _ => {}
        }
    }

    fn open_background_tab(&mut self, url: Url) {
        let id = self.next_view_id;
        self.next_view_id += 1;
        let mut view = WebView::new(id);
        self.open_url(&mut view, url, true);
        self.tabs.insert(self.active + 1, Tab { view });
    }

    // -------------------------------------------------------------- render

    fn render(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let scale = window.scale_factor();
        let (lw, lh) = (size.width as f64 / scale, size.height as f64 / scale);
        self.ui.set_size(lw, lh);
        let vp = self.viewport();
        let t = Affine::scale(scale);
        self.scene.reset();

        // conteúdo da aba ativa
        paint_content(
            &mut self.scene,
            &mut self.le,
            &mut self.tabs[self.active].view,
            self.ui.theme.dark,
            (lw, lh),
            t,
            vp,
        );
        // interface por cima
        self.with_model(|ui, m, le, scene| ui.paint(scene, le, m, t));

        let Some(gpu) = &mut self.gpu else { return };
        if gpu.surface.config.width != size.width || gpu.surface.config.height != size.height {
            gpu.cx
                .resize_surface(&mut gpu.surface, size.width, size.height);
        }
        let dev = &gpu.cx.devices[gpu.surface.dev_id];
        let params = RenderParams {
            base_color: Color::BLACK,
            width: size.width,
            height: size.height,
            antialiasing_method: AaConfig::Area,
        };
        if let Err(e) = gpu.renderer.render_to_texture(
            &dev.device,
            &dev.queue,
            &self.scene,
            &gpu.surface.target_view,
            &params,
        ) {
            eprintln!("erro ao desenhar: {e}");
            return;
        }
        let frame = match gpu.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                gpu.cx.configure_surface(&gpu.surface);
                window.request_redraw();
                return;
            }
            _ => return,
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = dev
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nous-blit"),
            });
        gpu.surface
            .blitter
            .copy(&dev.device, &mut encoder, &gpu.surface.target_view, &target);
        dev.queue.submit([encoder.finish()]);
        window.pre_present_notify();
        dev.queue.present(frame);
    }

    fn system_theme(&mut self) {
        if self.theme_override.is_some() {
            return;
        }
        let dark = self
            .window
            .as_ref()
            .and_then(|w| w.theme())
            .map(|t| t == winit::window::Theme::Dark)
            .unwrap_or(true);
        self.ui.theme = if dark { Theme::dark() } else { Theme::light() };
    }
}

impl ApplicationHandler<EngineEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Nous")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(520.0, 320.0))
            .with_decorations(false);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("não foi possível criar a janela: {e}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        // tenta cada backend na ordem de preferência até um funcionar
        let mut found = None;
        for backends in backend_candidates() {
            let mut cx = render_context(backends);
            match pollster::block_on(cx.create_surface(
                window.clone(),
                size.width.max(1),
                size.height.max(1),
                wgpu::PresentMode::AutoVsync,
            )) {
                Ok(surface) => {
                    found = Some((cx, surface));
                    break;
                }
                Err(e) => eprintln!("backend {backends:?} indisponível: {e}"),
            }
        }
        let Some((cx, surface)) = found else {
            eprintln!("nenhuma GPU compatível");
            event_loop.exit();
            return;
        };
        let renderer = Renderer::new(
            &cx.devices[surface.dev_id].device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        );
        let renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                eprintln!("falha ao iniciar o renderizador: {e}");
                event_loop.exit();
                return;
            }
        };
        self.gpu = Some(Gpu {
            cx,
            surface,
            renderer,
        });
        self.window = Some(window);
        self.system_theme();
        self.update_title();
        self.redraw();
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: EngineEvent) {
        match event {
            EngineEvent::Loaded { view, nav, page } => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.view.id == view) {
                    tab.view.on_loaded(nav, *page);
                }
                self.update_title();
                self.redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.render(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.redraw(),
            WindowEvent::ThemeChanged(_) => {
                self.system_theme();
                self.redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => self.on_key(event, event_loop),
            WindowEvent::CursorMoved { position, .. } => {
                let k = self.scale();
                self.on_cursor_moved(position.x / k, position.y / k, event_loop);
            }
            WindowEvent::CursorLeft { .. } => {
                if self.ui.mouse_leave() {
                    self.redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse(state, button, event_loop)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * 48.0,
                    MouseScrollDelta::PixelDelta(p) => (-p.y / self.scale()) as f32,
                };
                let vp = self.viewport();
                self.tabs[self.active].view.scroll_by(dy, vp.height);
                self.redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // só pede quadros contínuos enquanto há animação (carregamento)
        let animating = self.with_model(|ui, m, _, _| ui.animating(m));
        if animating {
            self.redraw();
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Ordem de backends de GPU. `WGPU_BACKEND` (ex.: `dx12`) força um específico.
///
/// No Windows o Vulkan vem primeiro: medido nesta fase, com example.com aberto, o Nous usou
/// ~54 MB (conjunto de trabalho privado) com Vulkan contra ~180 MB com DX12, e a diferença
/// é quase toda alocação do driver de vídeo. Se o Vulkan não existir, cai para DX12.
pub(crate) fn backend_candidates() -> Vec<wgpu::Backends> {
    if let Some(b) = wgpu::Backends::from_env() {
        return vec![b];
    }
    if cfg!(target_os = "windows") {
        vec![
            wgpu::Backends::VULKAN,
            wgpu::Backends::DX12,
            wgpu::Backends::all(),
        ]
    } else {
        vec![wgpu::Backends::all()]
    }
}

/// Igual a `RenderContext::new()`, mas com os backends escolhidos por nós.
pub(crate) fn render_context(backends: wgpu::Backends) -> RenderContext {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        display: None,
        backends,
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
    });
    RenderContext {
        instance,
        devices: Vec::new(),
    }
}

/// Desenha a página da aba (fundo, conteúdo recortado abaixo da interface).
pub(crate) fn paint_content(
    scene: &mut Scene,
    le: &mut LayoutEngine,
    view: &mut WebView,
    dark: bool,
    (lw, lh): (f64, f64),
    t: Affine,
    vp: PageViewport,
) {
    let content = Rect::new(0.0, CHROME_H, lw, lh);
    let fallback = if dark {
        [0x07, 0x08, 0x0d, 255]
    } else {
        [255, 255, 255, 255]
    };
    view.ensure_layout(le, vp);
    let bg = view
        .canvas_color()
        .map(rgba)
        .unwrap_or_else(|| to_color(fallback));
    scene.fill(Fill::NonZero, t, bg, None, &content);
    scene.push_clip_layer(Fill::NonZero, t, &content);
    view.paint(scene, t * Affine::translate((0.0, CHROME_H)), vp);
    scene.pop_layer();
}

fn main() {
    // modo de captura: renderiza fora da tela e salva PNG (testes e verificação)
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--captura") {
        let out = args.get(2).cloned().unwrap_or_else(|| "captura.png".into());
        let input = args.get(3).cloned().unwrap_or_else(|| NEW_TAB_URL.into());
        let dark = !args.iter().any(|a| a == "--claro");
        match capture::run(&input, std::path::Path::new(&out), 1280, 800, dark) {
            Ok(info) => {
                println!("{info}");
                return;
            }
            Err(e) => {
                eprintln!("falha na captura: {e}");
                std::process::exit(1);
            }
        }
    }

    let event_loop = match EventLoop::<EngineEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("não foi possível iniciar o loop de eventos: {e}");
            std::process::exit(1);
        }
    };
    let mut app = match App::new(event_loop.create_proxy()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("não foi possível iniciar o motor: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("erro no loop de eventos: {e}");
    }
}
