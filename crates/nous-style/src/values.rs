//! Valores computados de CSS que o layout e a pintura consomem.
//!
//! Esta struct é a fronteira estável entre o estilo e o resto do motor: quando o Stylo
//! entrar no lugar da cascata provisória, o layout continua lendo `ComputedStyle`.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);
    pub const BLACK: Rgba = Rgba::new(0, 0, 0, 255);
    pub const WHITE: Rgba = Rgba::new(255, 255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }

    pub fn is_transparent(self) -> bool {
        self.a == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
    Auto,
}

impl Length {
    pub fn resolve(self, base: f32) -> Option<f32> {
        match self {
            Length::Px(v) => Some(v),
            Length::Percent(p) => Some(base * p / 100.0),
            Length::Auto => None,
        }
    }

    pub fn resolve_or_zero(self, base: f32) -> f32 {
        self.resolve(base).unwrap_or(0.0)
    }

    pub fn is_auto(self) -> bool {
        matches!(self, Length::Auto)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    InlineBlock,
    ListItem,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Pre,
    NoWrap,
    PreWrap,
    PreLine,
}

impl WhiteSpace {
    pub fn collapses_spaces(self) -> bool {
        matches!(
            self,
            WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine
        )
    }
    pub fn keeps_newlines(self) -> bool {
        matches!(
            self,
            WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine
        )
    }
    pub fn wraps(self) -> bool {
        !matches!(self, WhiteSpace::Pre | WhiteSpace::NoWrap)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenericFamily {
    SansSerif,
    Serif,
    Monospace,
}

/// `font-family`: a lista original (para procurar fontes pelo nome) e a família genérica
/// usada como reserva.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFamily {
    pub names: Arc<[Arc<str>]>,
    pub generic: GenericFamily,
}

impl Default for FontFamily {
    fn default() -> Self {
        FontFamily {
            names: Arc::new([]),
            generic: GenericFamily::Serif,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    Normal,
    /// Múltiplo do tamanho da fonte.
    Number(f32),
    Px(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStyle {
    Disc,
    Circle,
    Square,
    Decimal,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Rgba,
    pub background_color: Rgba,
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    pub font_family: FontFamily,
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub white_space: WhiteSpace,
    pub underline: bool,
    pub line_through: bool,
    /// Ordem: topo, direita, baixo, esquerda (como no CSS).
    pub margin: [Length; 4],
    pub padding: [Length; 4],
    pub border_width: [f32; 4],
    pub border_color: [Rgba; 4],
    pub border_radius: f32,
    pub width: Length,
    pub height: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub list_style: ListStyle,
    pub visible: bool,
}

impl ComputedStyle {
    /// Valores iniciais definidos pela especificação CSS.
    pub fn initial() -> Self {
        ComputedStyle {
            display: Display::Inline,
            color: Rgba::BLACK,
            background_color: Rgba::TRANSPARENT,
            font_size: 16.0,
            font_weight: 400,
            italic: false,
            font_family: FontFamily::default(),
            line_height: LineHeight::Normal,
            text_align: TextAlign::Left,
            white_space: WhiteSpace::Normal,
            underline: false,
            line_through: false,
            margin: [Length::Px(0.0); 4],
            padding: [Length::Px(0.0); 4],
            border_width: [0.0; 4],
            border_color: [Rgba::BLACK; 4],
            border_radius: 0.0,
            width: Length::Auto,
            height: Length::Auto,
            min_height: Length::Auto,
            max_width: Length::Auto,
            list_style: ListStyle::Disc,
            visible: true,
        }
    }

    /// Começa um estilo filho: herda só as propriedades herdáveis do pai.
    pub fn inherit_from(parent: &ComputedStyle) -> Self {
        ComputedStyle {
            color: parent.color,
            font_size: parent.font_size,
            font_weight: parent.font_weight,
            italic: parent.italic,
            font_family: parent.font_family.clone(),
            line_height: parent.line_height,
            text_align: parent.text_align,
            white_space: parent.white_space,
            underline: parent.underline,
            line_through: parent.line_through,
            list_style: parent.list_style,
            visible: parent.visible,
            ..ComputedStyle::initial()
        }
    }

    pub fn line_height_px(&self) -> f32 {
        match self.line_height {
            LineHeight::Normal => self.font_size * 1.2,
            LineHeight::Number(n) => self.font_size * n,
            LineHeight::Px(px) => px,
        }
    }

    pub fn is_block_level(&self) -> bool {
        matches!(self.display, Display::Block | Display::ListItem)
    }
}
