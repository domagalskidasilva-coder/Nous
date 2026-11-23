//! Integração do Taffy (0.15) para Flexbox, InlineFlex, CSS Grid e tabelas básicas,
//! usando Parley para medir nós folha de texto e respeitando `box-sizing`, `min-width`,
//! `max-height`, `overflow: hidden` e `opacity`.

use std::sync::Arc;

use nous_dom::{NodeId, local_name};
use nous_style::{
    AlignItems, AlignSelf, BoxSizing, ComputedStyle, Display, FlexDirection, FlexWrap,
    GridPlacement, JustifyContent, Length, Overflow, Position, TrackBound, TrackSize,
};
use taffy::CoreStyle;
use taffy::prelude::{FromFr, FromLength, FromPercent, TaffyAuto, TaffyMaxContent, TaffyMinContent};
use taffy::style_helpers::{line, span};
use taffy::{
    AvailableSpace, Dimension, GridTemplateComponent, LayoutInput, LayoutOutput,
    LengthPercentage, LengthPercentageAuto, MaxTrackSizingFunction, MinTrackSizingFunction,
    TaffyTree, TrackSizingFunction, compute_leaf_layout,
};

type TaffyIdent = <taffy::Style as CoreStyle>::CustomIdent;

use crate::boxes::{BlockBox, BlockContent, BoxBuilder};
use crate::forms::{
    ControlInfo, extract_control_info, is_form_control, measure_control_content,
    paint_control_inner,
};
use crate::{
    ImageMap, LayoutCx, LayoutEngine, PaintItem, Rect, clip_rect_for, paint_img_content,
    resolve_border_radius,
};

pub(crate) enum TaffyNodeKind<'a> {
    Container,
    Table { rows: Vec<TableRowInfo> },
    BlockLeaf(&'a BlockBox),
    OwnedLeaf(BlockBox),
    ReplacedImg(NodeId),
    FormControl(NodeId, ControlInfo),
}

pub(crate) struct TableRowInfo {
    style: Arc<ComputedStyle>,
    cells: Vec<taffy::NodeId>,
}

pub(crate) struct TaffyNodeData<'a> {
    style: Arc<ComputedStyle>,
    kind: TaffyNodeKind<'a>,
}

fn to_length_pct(l: Length) -> LengthPercentage {
    match l {
        Length::Px(px) => LengthPercentage::length(px),
        Length::Percent(p) => LengthPercentage::percent(p / 100.0),
        Length::Auto => LengthPercentage::length(0.0),
    }
}

fn to_length_pct_auto(l: Length) -> LengthPercentageAuto {
    match l {
        Length::Px(px) => LengthPercentageAuto::length(px),
        Length::Percent(p) => LengthPercentageAuto::percent(p / 100.0),
        Length::Auto => LengthPercentageAuto::AUTO,
    }
}

fn to_dimension(l: Length) -> Dimension {
    match l {
        Length::Px(px) => Dimension::length(px),
        Length::Percent(p) => Dimension::percent(p / 100.0),
        Length::Auto => Dimension::AUTO,
    }
}

fn to_overflow(o: Overflow) -> taffy::Overflow {
    match o {
        Overflow::Visible => taffy::Overflow::Visible,
        Overflow::Hidden => taffy::Overflow::Hidden,
        Overflow::Scroll | Overflow::Auto => taffy::Overflow::Scroll,
    }
}

fn to_align_items(a: AlignItems) -> taffy::AlignItems {
    match a {
        AlignItems::Stretch => taffy::AlignItems::STRETCH,
        AlignItems::FlexStart => taffy::AlignItems::FLEX_START,
        AlignItems::FlexEnd => taffy::AlignItems::FLEX_END,
        AlignItems::Center => taffy::AlignItems::CENTER,
        AlignItems::Baseline => taffy::AlignItems::BASELINE,
    }
}

fn to_align_self(a: AlignSelf) -> Option<taffy::AlignSelf> {
    Some(match a {
        AlignSelf::Auto => return None,
        AlignSelf::Stretch => taffy::AlignItems::STRETCH,
        AlignSelf::FlexStart => taffy::AlignItems::FLEX_START,
        AlignSelf::FlexEnd => taffy::AlignItems::FLEX_END,
        AlignSelf::Center => taffy::AlignItems::CENTER,
        AlignSelf::Baseline => taffy::AlignItems::BASELINE,
    })
}

fn to_justify_content(j: JustifyContent) -> taffy::JustifyContent {
    match j {
        JustifyContent::FlexStart => taffy::JustifyContent::FLEX_START,
        JustifyContent::FlexEnd => taffy::JustifyContent::FLEX_END,
        JustifyContent::Center => taffy::JustifyContent::CENTER,
        JustifyContent::SpaceBetween => taffy::JustifyContent::SPACE_BETWEEN,
        JustifyContent::SpaceAround => taffy::JustifyContent::SPACE_AROUND,
        JustifyContent::SpaceEvenly => taffy::JustifyContent::SPACE_EVENLY,
        JustifyContent::Stretch => taffy::JustifyContent::STRETCH,
    }
}

fn to_min_track(b: TrackBound) -> MinTrackSizingFunction {
    match b {
        TrackBound::Px(px) => MinTrackSizingFunction::length(px),
        TrackBound::Percent(p) => MinTrackSizingFunction::percent(p / 100.0),
        TrackBound::Fr(_) | TrackBound::Auto => MinTrackSizingFunction::AUTO,
        TrackBound::MinContent => MinTrackSizingFunction::MIN_CONTENT,
        TrackBound::MaxContent => MinTrackSizingFunction::MAX_CONTENT,
    }
}

fn to_max_track(b: TrackBound) -> MaxTrackSizingFunction {
    match b {
        TrackBound::Px(px) => MaxTrackSizingFunction::length(px),
        TrackBound::Percent(p) => MaxTrackSizingFunction::percent(p / 100.0),
        TrackBound::Fr(f) => MaxTrackSizingFunction::fr(f),
        TrackBound::Auto => MaxTrackSizingFunction::AUTO,
        TrackBound::MinContent => MaxTrackSizingFunction::MIN_CONTENT,
        TrackBound::MaxContent => MaxTrackSizingFunction::MAX_CONTENT,
    }
}

fn to_track(t: TrackSize) -> GridTemplateComponent<TaffyIdent> {
    let f: TrackSizingFunction = match t {
        TrackSize::Px(px) => TrackSizingFunction::from_length(px),
        TrackSize::Percent(p) => TrackSizingFunction::from_percent(p / 100.0),
        TrackSize::Fr(f) => TrackSizingFunction::from_fr(f),
        TrackSize::Auto => TrackSizingFunction::AUTO,
        TrackSize::MinContent => TrackSizingFunction::MIN_CONTENT,
        TrackSize::MaxContent => TrackSizingFunction::MAX_CONTENT,
        TrackSize::MinMax(a, b) => TrackSizingFunction {
            min: to_min_track(a),
            max: to_max_track(b),
        },
    };
    GridTemplateComponent::Single(f)
}

fn to_grid_line(p: GridPlacement) -> taffy::Line<taffy::GridPlacement<TaffyIdent>> {
    let start = match p.start {
        Some(n) => line(n),
        None => taffy::GridPlacement::Auto,
    };
    let end = match p.span {
        Some(s) => span(s),
        None => taffy::GridPlacement::Auto,
    };
    taffy::Line { start, end }
}

fn build_taffy_style(s: &ComputedStyle, is_root: bool) -> taffy::Style {
    let display = match s.display {
        Display::Flex | Display::InlineFlex => taffy::Display::Flex,
        Display::Grid | Display::Table => taffy::Display::Grid,
        Display::None => taffy::Display::None,
        _ => taffy::Display::Block,
    };
    let box_sizing = match s.box_sizing {
        BoxSizing::BorderBox => taffy::BoxSizing::BorderBox,
        BoxSizing::ContentBox => taffy::BoxSizing::ContentBox,
    };
    let mut style = taffy::Style {
        display,
        box_sizing,
        overflow: taffy::Point {
            x: to_overflow(s.overflow_x),
            y: to_overflow(s.overflow_y),
        },
        position: if is_root {
            taffy::Position::Static
        } else {
            match s.position {
                Position::Static => taffy::Position::Static,
                Position::Relative => taffy::Position::Relative,
                Position::Absolute => taffy::Position::Absolute,
                Position::Fixed => taffy::Position::Fixed,
            }
        },
        inset: if is_root {
            taffy::Rect::auto()
        } else {
            taffy::Rect {
                top: to_length_pct_auto(s.inset[0]),
                right: to_length_pct_auto(s.inset[1]),
                bottom: to_length_pct_auto(s.inset[2]),
                left: to_length_pct_auto(s.inset[3]),
            }
        },
        margin: if is_root {
            taffy::Rect::zero()
        } else {
            taffy::Rect {
                top: to_length_pct_auto(s.margin[0]),
                right: to_length_pct_auto(s.margin[1]),
                bottom: to_length_pct_auto(s.margin[2]),
                left: to_length_pct_auto(s.margin[3]),
            }
        },
        padding: taffy::Rect {
            top: to_length_pct(s.padding[0]),
            right: to_length_pct(s.padding[1]),
            bottom: to_length_pct(s.padding[2]),
            left: to_length_pct(s.padding[3]),
        },
        border: taffy::Rect {
            top: LengthPercentage::length(s.border_width[0]),
            right: LengthPercentage::length(s.border_width[1]),
            bottom: LengthPercentage::length(s.border_width[2]),
            left: LengthPercentage::length(s.border_width[3]),
        },
        size: taffy::Size {
            width: to_dimension(s.width),
            height: to_dimension(s.height),
        },
        min_size: taffy::Size {
            width: to_length_pct_auto(s.min_width),
            height: to_length_pct_auto(s.min_height),
        },
        max_size: taffy::Size {
            width: to_length_pct_auto(s.max_width),
            height: to_length_pct_auto(s.max_height),
        },
        flex_direction: match s.flex_direction {
            FlexDirection::Row => taffy::FlexDirection::Row,
            FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
            FlexDirection::Column => taffy::FlexDirection::Column,
            FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
        },
        flex_wrap: match s.flex_wrap {
            FlexWrap::NoWrap => taffy::FlexWrap::NoWrap,
            FlexWrap::Wrap => taffy::FlexWrap::Wrap,
            FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        },
        justify_content: to_justify_content(s.justify_content),
        align_items: to_align_items(s.align_items),
        align_self: to_align_self(s.align_self),
        flex_grow: s.flex_grow,
        flex_shrink: s.flex_shrink,
        flex_basis: to_dimension(s.flex_basis),
        gap: taffy::Size {
            width: to_length_pct(s.column_gap),
            height: to_length_pct(s.row_gap),
        },
        grid_template_columns: s
            .grid_template_columns
            .iter()
            .copied()
            .map(to_track)
            .collect(),
        grid_template_rows: s
            .grid_template_rows
            .iter()
            .copied()
            .map(to_track)
            .collect(),
        grid_column: to_grid_line(s.grid_column),
        grid_row: to_grid_line(s.grid_row),
        ..Default::default()
    };
    if s.display == Display::Table {
        style.item_is_table = true;
    }
    style
}

fn collect_table_rows<'b>(
    builder: &BoxBuilder<'_>,
    blocks: &'b [BlockBox],
    out: &mut Vec<(Arc<ComputedStyle>, Vec<&'b BlockBox>)>,
) {
    let mut stray_cells: Vec<&'b BlockBox> = Vec::new();
    let flush_stray = |stray: &mut Vec<&'b BlockBox>,
                       out: &mut Vec<(Arc<ComputedStyle>, Vec<&'b BlockBox>)>| {
        if !stray.is_empty() {
            let st = stray[0].style.clone();
            out.push((st, std::mem::take(stray)));
        }
    };
    for child in blocks {
        if child.style.display == Display::TableRow {
            flush_stray(&mut stray_cells, out);
            let cells: Vec<&BlockBox> = match &child.content {
                BlockContent::Blocks(cs) => cs.iter().collect(),
                _ => Vec::new(),
            };
            out.push((child.style.clone(), cells));
        } else if child.style.display == Display::TableCell {
            stray_cells.push(child);
        } else if let Some(n) = child.node
            && builder
                .doc
                .element(n)
                .is_some_and(|e| matches!(&**e.local_name(), "thead" | "tbody" | "tfoot"))
        {
            flush_stray(&mut stray_cells, out);
            if let BlockContent::Blocks(inner) = &child.content {
                collect_table_rows(builder, inner, out);
            }
        } else {
            flush_stray(&mut stray_cells, out);
            if let BlockContent::Blocks(inner) = &child.content {
                let has_rows = inner.iter().any(|c| c.style.display == Display::TableRow);
                if has_rows {
                    collect_table_rows(builder, inner, out);
                }
            }
        }
    }
    flush_stray(&mut stray_cells, out);
}

fn build_taffy_node<'b>(
    builder: &BoxBuilder<'_>,
    images: &ImageMap,
    tree: &mut TaffyTree<TaffyNodeData<'b>>,
    b: &'b BlockBox,
    is_root: bool,
    root_width_override: Option<f32>,
) -> taffy::NodeId {
    let doc = builder.doc;
    let s = &b.style;
    let mut taffy_style = build_taffy_style(s, is_root);
    if is_root && let Some(w) = root_width_override {
        taffy_style.size.width = Dimension::length(w);
    }

    if let Some(node) = b.node
        && let Some(el) = doc.element(node)
    {
        if el.is(&local_name!("img")) {
            let img = images.get(&node);
            if let Some(im) = img
                && im.width > 0
                && im.height > 0
            {
                taffy_style.item_is_replaced = true;
                taffy_style.aspect_ratio = Some(im.width as f32 / im.height as f32);
            }
            return tree
                .new_leaf_with_context(
                    taffy_style,
                    TaffyNodeData {
                        style: s.clone(),
                        kind: TaffyNodeKind::ReplacedImg(node),
                    },
                )
                .expect("taffy leaf");
        }
        if is_form_control(el)
            && let Some(info) = extract_control_info(doc, node)
        {
            taffy_style.item_is_compressible_replaced = true;
            return tree
                .new_leaf_with_context(
                    taffy_style,
                    TaffyNodeData {
                        style: s.clone(),
                        kind: TaffyNodeKind::FormControl(node, info),
                    },
                )
                .expect("taffy leaf");
        }
    }

    if s.display == Display::Table {
        let mut raw_rows = Vec::new();
        if let BlockContent::Blocks(children) = &b.content {
            collect_table_rows(builder, children, &mut raw_rows);
        }
        let mut max_cols = 0usize;
        for (_, cells) in &raw_rows {
            let mut cols = 0usize;
            for cell in cells {
                let colspan = cell
                    .node
                    .and_then(|n| doc.element(n))
                    .and_then(|e| e.attr("colspan"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                cols += colspan;
            }
            max_cols = max_cols.max(cols);
        }
        taffy_style.display = taffy::Display::Grid;
        taffy_style.grid_template_columns =
            vec![GridTemplateComponent::Single(TrackSizingFunction::AUTO); max_cols];
        if taffy_style.gap.width == LengthPercentage::length(0.0)
            && taffy_style.gap.height == LengthPercentage::length(0.0)
        {
            taffy_style.gap = taffy::Size {
                width: LengthPercentage::length(2.0),
                height: LengthPercentage::length(2.0),
            };
        }

        let mut child_ids = Vec::new();
        let mut rows_info = Vec::with_capacity(raw_rows.len());
        for (r_idx, (row_style, cells)) in raw_rows.into_iter().enumerate() {
            let mut col_cursor = 1i16;
            let mut row_cells = Vec::with_capacity(cells.len());
            for cell in cells {
                let colspan = cell
                    .node
                    .and_then(|n| doc.element(n))
                    .and_then(|e| e.attr("colspan"))
                    .and_then(|v| v.trim().parse::<u16>().ok())
                    .unwrap_or(1)
                    .clamp(1, 64);
                let cid = build_taffy_node(builder, images, tree, cell, false, None);
                if let Ok(mut cs) = tree.style(cid).cloned() {
                    cs.grid_row = taffy::Line {
                        start: line(r_idx as i16 + 1),
                        end: taffy::GridPlacement::Auto,
                    };
                    cs.grid_column = taffy::Line {
                        start: line(col_cursor),
                        end: if colspan > 1 {
                            span(colspan)
                        } else {
                            taffy::GridPlacement::Auto
                        },
                    };
                    let _ = tree.set_style(cid, cs);
                }
                col_cursor += colspan as i16;
                child_ids.push(cid);
                row_cells.push(cid);
            }
            rows_info.push(TableRowInfo {
                style: row_style,
                cells: row_cells,
            });
        }
        let nid = tree
            .new_with_children(taffy_style, &child_ids)
            .expect("taffy table");
        let _ = tree.set_node_context(
            nid,
            Some(TaffyNodeData {
                style: s.clone(),
                kind: TaffyNodeKind::Table { rows: rows_info },
            }),
        );
        return nid;
    }

    if matches!(
        s.display,
        Display::Flex | Display::InlineFlex | Display::Grid
    ) {
        let mut child_ids = Vec::new();
        match &b.content {
            BlockContent::Blocks(children) => {
                for c in children {
                    child_ids.push(build_taffy_node(builder, images, tree, c, false, None));
                }
            }
            BlockContent::Inline(nodes) => {
                let anon = BlockBox {
                    node: None,
                    style: Arc::new(ComputedStyle::inherit_from(s)),
                    content: BlockContent::Inline(nodes.clone()),
                };
                let cid = tree
                    .new_leaf_with_context(
                        taffy::Style::default(),
                        TaffyNodeData {
                            style: anon.style.clone(),
                            kind: TaffyNodeKind::OwnedLeaf(anon),
                        },
                    )
                    .expect("taffy inline anon");
                child_ids.push(cid);
            }
            BlockContent::Empty => {}
        }
        let nid = tree
            .new_with_children(taffy_style, &child_ids)
            .expect("taffy container");
        let _ = tree.set_node_context(
            nid,
            Some(TaffyNodeData {
                style: s.clone(),
                kind: TaffyNodeKind::Container,
            }),
        );
        return nid;
    }

    tree.new_leaf_with_context(
        taffy_style,
        TaffyNodeData {
            style: s.clone(),
            kind: TaffyNodeKind::BlockLeaf(b),
        },
    )
    .expect("taffy block leaf")
}

fn measure_block_content<'a>(
    builder: &BoxBuilder<'a>,
    engine: &mut LayoutEngine,
    images: &ImageMap,
    viewport_width: f32,
    b: &BlockBox,
    avail_w: AvailableSpace,
) -> taffy::Size<f32> {
    let mut cx = LayoutCx::new(
        builder,
        engine,
        images,
        builder.doc.document_element().unwrap_or(NodeId::DOCUMENT),
        viewport_width,
    );
    let (w, h) = cx.measure_block_content_size(b, avail_w);
    taffy::Size {
        width: w,
        height: h,
    }
}

fn measure_taffy_node<'b>(
    builder: &BoxBuilder<'_>,
    engine: &mut LayoutEngine,
    images: &ImageMap,
    viewport_width: f32,
    inputs: LayoutInput,
    ctx: Option<&mut TaffyNodeData<'b>>,
    style: &taffy::Style,
) -> LayoutOutput {
    compute_leaf_layout(inputs, style, |_, _| 0.0, |_, avail| {
        let Some(data) = ctx else {
            return taffy::Size::ZERO;
        };
        let avail_w_opt = match avail.width {
            AvailableSpace::Definite(w) => Some(w),
            AvailableSpace::MinContent => Some(1.0),
            AvailableSpace::MaxContent => None,
        };
        match &data.kind {
            TaffyNodeKind::ReplacedImg(node) => {
                if let Some(im) = images.get(node)
                    && im.width > 0
                    && im.height > 0
                {
                    return taffy::Size {
                        width: im.width as f32,
                        height: im.height as f32,
                    };
                }
                if let Some(el) = builder.doc.element(*node)
                    && let Some(alt) = el.attr("alt").filter(|a| !a.trim().is_empty())
                {
                    let mut dummy = LayoutCx::new(
                        builder,
                        engine,
                        images,
                        builder.doc.document_element().unwrap_or(*node),
                        viewport_width,
                    );
                    let l = dummy.build_text_layout(&data.style, alt, &[], &[], avail_w_opt);
                    return taffy::Size {
                        width: l.full_width(),
                        height: l.height(),
                    };
                }
                taffy::Size::ZERO
            }
            TaffyNodeKind::FormControl(_, info) => {
                let (w, h) = measure_control_content(engine, info, &data.style, avail_w_opt);
                taffy::Size {
                    width: w,
                    height: h,
                }
            }
            TaffyNodeKind::BlockLeaf(b) => {
                measure_block_content(builder, engine, images, viewport_width, b, avail.width)
            }
            TaffyNodeKind::OwnedLeaf(b) => {
                measure_block_content(builder, engine, images, viewport_width, b, avail.width)
            }
            TaffyNodeKind::Container | TaffyNodeKind::Table { .. } => taffy::Size::ZERO,
        }
    })
}

pub(crate) fn layout_taffy_container<'a, 'b>(
    cx: &mut LayoutCx<'a, '_>,
    b: &'b BlockBox,
    x: f32,
    y: f32,
    avail_w: f32,
    width_override: Option<f32>,
    paint: bool,
) -> (f32, f32) {
    let mut tree: TaffyTree<TaffyNodeData<'b>> = TaffyTree::new();
    let root_id = build_taffy_node(cx.builder, cx.images, &mut tree, b, true, width_override);
    let builder = cx.builder;
    let images = cx.images;
    let viewport_width = cx.viewport_width;
    let engine = &mut *cx.engine;

    let _ = tree.compute_layout_with_measure(
        root_id,
        taffy::Size {
            width: AvailableSpace::Definite(avail_w.max(0.0)),
            height: AvailableSpace::MaxContent,
        },
        |inputs, _nid, ctx, style| {
            measure_taffy_node(builder, engine, images, viewport_width, inputs, ctx, style)
        },
    );

    let root_layout = *tree.layout(root_id).expect("root layout");
    if paint {
        paint_taffy_node(cx, &tree, root_id, x, y);
    }
    (root_layout.size.width, root_layout.size.height)
}

fn paint_taffy_node<'a, 'b>(
    cx: &mut LayoutCx<'a, '_>,
    tree: &TaffyTree<TaffyNodeData<'b>>,
    nid: taffy::NodeId,
    x: f32,
    y: f32,
) {
    let tl = tree.layout(nid).expect("taffy node layout");
    let Some(data) = tree.get_node_context(nid) else {
        return;
    };
    let s = &data.style;
    let border_box = Rect::new(x, y, tl.size.width, tl.size.height);
    let bw = [
        tl.border.top,
        tl.border.right,
        tl.border.bottom,
        tl.border.left,
    ];
    let padding_box = Rect::new(
        x + bw[3],
        y + bw[0],
        (border_box.w - bw[1] - bw[3]).max(0.0),
        (border_box.h - bw[0] - bw[2]).max(0.0),
    );
    let content_box = Rect::new(
        x + tl.border.left + tl.padding.left,
        y + tl.border.top + tl.padding.top,
        tl.content_box_width().max(0.0),
        tl.content_box_height().max(0.0),
    );

    let pushed_opacity = s.opacity < 1.0;
    if pushed_opacity {
        cx.items
            .push(PaintItem::PushOpacity(s.opacity.clamp(0.0, 1.0)));
    }
    if s.visible && !s.background_color.is_transparent() {
        cx.items.push(PaintItem::Rect {
            rect: border_box,
            color: s.background_color,
            radius: resolve_border_radius(s, border_box),
        });
    }
    if s.visible && bw.iter().any(|&w| w > 0.0) {
        cx.items.push(PaintItem::Border {
            rect: border_box,
            widths: bw,
            colors: s.border_color,
        });
    }
    let clip_rect = clip_rect_for(s, padding_box);
    if let Some(cr) = clip_rect {
        cx.items.push(PaintItem::PushClip(cr));
    }

    match &data.kind {
        TaffyNodeKind::Container => {
            if let Ok(children) = tree.children(nid) {
                for cid in children {
                    let cl = tree.layout(cid).expect("child layout");
                    paint_taffy_node(cx, tree, cid, x + cl.location.x, y + cl.location.y);
                }
            }
        }
        TaffyNodeKind::Table { rows } => {
            for row in rows {
                if row.cells.is_empty() {
                    continue;
                }
                let mut min_x = f32::INFINITY;
                let mut min_y = f32::INFINITY;
                let mut max_x = f32::NEG_INFINITY;
                let mut max_y = f32::NEG_INFINITY;
                for &cid in &row.cells {
                    if let Ok(cl) = tree.layout(cid) {
                        min_x = min_x.min(x + cl.location.x);
                        min_y = min_y.min(y + cl.location.y);
                        max_x = max_x.max(x + cl.location.x + cl.size.width);
                        max_y = max_y.max(y + cl.location.y + cl.size.height);
                    }
                }
                if min_x.is_finite() {
                    let row_rect = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);
                    if row.style.visible && !row.style.background_color.is_transparent() {
                        cx.items.push(PaintItem::Rect {
                            rect: row_rect,
                            color: row.style.background_color,
                            radius: resolve_border_radius(&row.style, row_rect),
                        });
                    }
                }
            }
            if let Ok(children) = tree.children(nid) {
                for cid in children {
                    let cl = tree.layout(cid).expect("cell layout");
                    paint_taffy_node(cx, tree, cid, x + cl.location.x, y + cl.location.y);
                }
            }
        }
        TaffyNodeKind::ReplacedImg(node) => {
            paint_img_content(cx, *node, s, content_box, border_box);
        }
        TaffyNodeKind::FormControl(node, info) => {
            paint_control_inner(cx, *node, info, s, border_box, content_box);
        }
        TaffyNodeKind::BlockLeaf(b) => {
            cx.paint_block_content_at(
                b,
                content_box.x,
                content_box.y,
                content_box.w,
                border_box.w,
            );
        }
        TaffyNodeKind::OwnedLeaf(b) => {
            cx.paint_block_content_at(
                b,
                content_box.x,
                content_box.y,
                content_box.w,
                border_box.w,
            );
        }
    }

    if clip_rect.is_some() {
        cx.items.push(PaintItem::PopClip);
    }
    if pushed_opacity {
        cx.items.push(PaintItem::PopOpacity);
    }
}
