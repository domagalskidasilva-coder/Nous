//! `nous --captura saida.png [url]`: carrega a página, desenha a janela inteira fora da
//! tela (mesmo caminho de pintura da janela de verdade) e salva um PNG. Serve para testes
//! visuais automáticos e para conferir o motor sem abrir janela.

use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nous_engine::{Engine, EngineEvent, PageViewport, SearchEngine, Url, WebView, resolve_input};
use nous_paint::LayoutEngine;
use nous_ui::{CHROME_H, TabView, Theme, Ui, UiModel};
use vello::kurbo::Affine;
use vello::peniko::Color;

use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

pub fn run(input: &str, out: &Path, width: u32, height: u32, dark: bool) -> Result<String, String> {
    let started = Instant::now();
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let engine = Engine::new(move |ev| {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(ev);
        }
    })?;
    let mut le = LayoutEngine::new();
    let mut ui = Ui::new(if dark { Theme::dark() } else { Theme::light() });
    let mut view = WebView::new(1);

    let url = resolve_input(input, SearchEngine::DuckDuckGo);
    if url.scheme() == "nous" {
        let html = match url.host_str() {
            Some("sobre") => crate::pages::about(),
            Some("memoria") => crate::pages::memory(&[], nous_engine::process_memory_bytes()),
            _ => crate::pages::new_tab(),
        };
        view.show_internal(url.clone(), &html, true);
    } else {
        view.navigate(&engine, url.clone());
        match rx.recv_timeout(Duration::from_secs(45)) {
            Ok(EngineEvent::Loaded { nav, page, .. }) => {
                view.on_loaded(nav, *page);
            }
            Err(_) => return Err(format!("tempo esgotado carregando {url}")),
        }
    }
    let loaded_ms = started.elapsed().as_millis();

    // monta a cena exatamente como a janela faria
    let (lw, lh) = (width as f64, height as f64);
    ui.set_size(lw, lh);
    let vp = PageViewport {
        width: lw as f32,
        height: (lh - CHROME_H) as f32,
    };
    let mut scene = Scene::new();
    let t = Affine::IDENTITY;
    let layout_start = Instant::now();
    crate::paint_content(&mut scene, &mut le, &mut view, dark, (lw, lh), t, vp);
    let layout_ms = layout_start.elapsed().as_millis();
    let title = view.title();
    let tabs = [TabView {
        title: &title,
        loading: false,
    }];
    let shown_url = match view.url() {
        Some(u) if u.scheme() == "nous" && u.host_str() == Some("nova-aba") => String::new(),
        Some(u) => u.to_string(),
        None => String::new(),
    };
    let model = UiModel {
        tabs: &tabs,
        active: 0,
        url: &shown_url,
        can_back: false,
        can_forward: false,
        loading: false,
        maximized: false,
        search_name: SearchEngine::DuckDuckGo.name(),
    };
    ui.paint(&mut scene, &mut le, &model, t);

    // GPU sem janela
    let mut found = None;
    for backends in crate::backend_candidates() {
        let mut cx = crate::render_context(backends);
        if let Some(id) = pollster::block_on(cx.device(None)) {
            found = Some((cx, id));
            break;
        }
    }
    let (cx, dev_id) = found.ok_or("nenhuma GPU compatível")?;
    let device = &cx.devices[dev_id].device;
    let queue = &cx.devices[dev_id].queue;
    let mut renderer = Renderer::new(
        device,
        RendererOptions {
            use_cpu: false,
            antialiasing_support: AaSupport::area_only(),
            num_init_threads: std::num::NonZeroUsize::new(1),
            pipeline_cache: None,
        },
    )
    .map_err(|e| e.to_string())?;
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nous-captura"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer
        .render_to_texture(
            device,
            queue,
            &scene,
            &target,
            &RenderParams {
                base_color: Color::BLACK,
                width,
                height,
                antialiasing_method: AaConfig::Area,
            },
        )
        .map_err(|e| e.to_string())?;

    // copia a textura para um buffer legível (linhas alinhadas em 256 bytes)
    let row = width * 4;
    let padded = row.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nous-captura-leitura"),
        size: padded as u64 * height as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("nous-captura-copia"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let (map_tx, map_rx) = mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = map_tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    map_rx
        .recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let mut pixels = Vec::with_capacity((row * height) as usize);
    {
        let data = buffer.get_mapped_range(..).map_err(|e| format!("{e:?}"))?;
        for y in 0..height as usize {
            let start = y * padded as usize;
            pixels.extend_from_slice(&data[start..start + row as usize]);
        }
    }
    buffer.unmap();

    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&pixels))
        .map_err(|e| e.to_string())?;

    let mem = view.memory();
    let process = nous_engine::process_memory_bytes()
        .map(|b| format!("{:.1} MB", b as f64 / 1048576.0))
        .unwrap_or_else(|| "?".into());
    let timings = view
        .page()
        .map(|p| {
            format!(
                "rede {:.0} ms, parse {:.1} ms, estilo {:.1} ms",
                p.timings.fetch_ms, p.timings.parse_ms, p.timings.style_ms
            )
        })
        .unwrap_or_default();
    let final_url: Option<&Url> = view.url();
    Ok(format!(
        "{}\n  título: {title}\n  url: {}\n  carregado em {loaded_ms} ms ({timings}), layout+pintura {layout_ms} ms\n  DOM {} nós, altura {:.0} px\n  RAM do processo: {process}",
        out.display(),
        final_url.map(|u| u.as_str()).unwrap_or(""),
        mem.dom_nodes,
        view.content_height(),
    ))
}
