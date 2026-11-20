//! Cores CSS: `#hex`, `rgb()`/`rgba()`, `hsl()`/`hsla()` e nomes.

use crate::values::Rgba;

pub fn parse_color(input: &str) -> Option<Rgba> {
    let s = input.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    let lower = s.to_ascii_lowercase();
    // light-dark(claro, escuro): por enquanto as páginas são sempre renderizadas no esquema claro
    if let Some(args) = function_args(&lower, &["light-dark"]) {
        let mut depth = 0;
        let split = args.char_indices().find(|&(_, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            c == ',' && depth == 0
        });
        let light = split.map(|(i, _)| &args[..i]).unwrap_or(args);
        return parse_color(light);
    }
    if let Some(args) = function_args(&lower, &["rgb", "rgba"]) {
        return parse_rgb(args);
    }
    if let Some(args) = function_args(&lower, &["hsl", "hsla"]) {
        return parse_hsl(args);
    }
    named(&lower)
}

fn function_args<'a>(s: &'a str, names: &[&str]) -> Option<&'a str> {
    for n in names {
        if let Some(rest) = s.strip_prefix(n)
            && let Some(rest) = rest.trim_start().strip_prefix('(')
        {
            return rest.trim_end().strip_suffix(')');
        }
    }
    None
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&hex[i..=i], 16).ok();
    let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 | 4 => {
            let r = digit(0)? * 17;
            let g = digit(1)? * 17;
            let b = digit(2)? * 17;
            let a = if hex.len() == 4 { digit(3)? * 17 } else { 255 };
            Some(Rgba::new(r, g, b, a))
        }
        6 | 8 => {
            let a = if hex.len() == 8 { pair(6)? } else { 255 };
            Some(Rgba::new(pair(0)?, pair(2)?, pair(4)?, a))
        }
        _ => None,
    }
}

/// Aceita a sintaxe antiga (`1, 2, 3, .5`) e a nova (`1 2 3 / 50%`).
fn split_args(args: &str) -> Vec<&str> {
    args.split([',', ' ', '/'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn channel(v: &str) -> Option<u8> {
    let f = if let Some(p) = v.strip_suffix('%') {
        p.parse::<f32>().ok()? * 2.55
    } else {
        v.parse::<f32>().ok()?
    };
    Some(f.round().clamp(0.0, 255.0) as u8)
}

fn alpha(v: Option<&&str>) -> Option<u8> {
    let Some(v) = v else { return Some(255) };
    let f = if let Some(p) = v.strip_suffix('%') {
        p.parse::<f32>().ok()? / 100.0
    } else {
        v.parse::<f32>().ok()?
    };
    Some((f.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn parse_rgb(args: &str) -> Option<Rgba> {
    let parts = split_args(args);
    if parts.len() < 3 {
        return None;
    }
    Some(Rgba::new(
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        alpha(parts.get(3))?,
    ))
}

fn parse_hsl(args: &str) -> Option<Rgba> {
    let parts = split_args(args);
    if parts.len() < 3 {
        return None;
    }
    let h = parts[0].trim_end_matches("deg").parse::<f32>().ok()?;
    let s = parts[1].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
    let l = parts[2].trim_end_matches('%').parse::<f32>().ok()? / 100.0;
    let (r, g, b) = hsl_to_rgb(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    Some(Rgba::new(r, g, b, alpha(parts.get(3))?))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0) / 360.0;
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    (hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0))
}

fn named(name: &str) -> Option<Rgba> {
    let hex: u32 = match name {
        "transparent" => return Some(Rgba::TRANSPARENT),
        "black" => 0x000000,
        "white" => 0xffffff,
        "red" => 0xff0000,
        "green" => 0x008000,
        "blue" => 0x0000ff,
        "yellow" => 0xffff00,
        "cyan" | "aqua" => 0x00ffff,
        "magenta" | "fuchsia" => 0xff00ff,
        "gray" | "grey" => 0x808080,
        "silver" => 0xc0c0c0,
        "maroon" => 0x800000,
        "olive" => 0x808000,
        "lime" => 0x00ff00,
        "teal" => 0x008080,
        "navy" => 0x000080,
        "purple" => 0x800080,
        "orange" => 0xffa500,
        "pink" => 0xffc0cb,
        "brown" => 0xa52a2a,
        "gold" => 0xffd700,
        "indigo" => 0x4b0082,
        "violet" => 0xee82ee,
        "darkgray" | "darkgrey" => 0xa9a9a9,
        "dimgray" | "dimgrey" => 0x696969,
        "lightgray" | "lightgrey" => 0xd3d3d3,
        "gainsboro" => 0xdcdcdc,
        "whitesmoke" => 0xf5f5f5,
        "darkblue" => 0x00008b,
        "darkgreen" => 0x006400,
        "darkred" => 0x8b0000,
        "lightblue" => 0xadd8e6,
        "lightgreen" => 0x90ee90,
        "lightyellow" => 0xffffe0,
        "steelblue" => 0x4682b4,
        "royalblue" => 0x4169e1,
        "dodgerblue" => 0x1e90ff,
        "skyblue" => 0x87ceeb,
        "slategray" | "slategrey" => 0x708090,
        "crimson" => 0xdc143c,
        "tomato" => 0xff6347,
        "coral" => 0xff7f50,
        "salmon" => 0xfa8072,
        "orangered" => 0xff4500,
        "darkorange" => 0xff8c00,
        "khaki" => 0xf0e68c,
        "beige" => 0xf5f5dc,
        "ivory" => 0xfffff0,
        "linen" => 0xfaf0e6,
        "lavender" => 0xe6e6fa,
        "aliceblue" => 0xf0f8ff,
        "azure" => 0xf0ffff,
        "honeydew" => 0xf0fff0,
        "mintcream" => 0xf5fffa,
        "snow" => 0xfffafa,
        "seashell" => 0xfff5ee,
        "wheat" => 0xf5deb3,
        "tan" => 0xd2b48c,
        "chocolate" => 0xd2691e,
        "firebrick" => 0xb22222,
        "seagreen" => 0x2e8b57,
        "forestgreen" => 0x228b22,
        "darkslategray" | "darkslategrey" => 0x2f4f4f,
        "midnightblue" => 0x191970,
        "darkviolet" => 0x9400d3,
        "rebeccapurple" => 0x663399,
        _ => return None,
    };
    Some(Rgba::new(
        (hex >> 16) as u8,
        (hex >> 8) as u8,
        hex as u8,
        255,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatos() {
        assert_eq!(parse_color("#fff"), Some(Rgba::WHITE));
        assert_eq!(
            parse_color("#7C5CFF"),
            Some(Rgba::new(0x7c, 0x5c, 0xff, 255))
        );
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Rgba::new(1, 2, 3, 255)));
        assert_eq!(
            parse_color("rgba(1,2,3,0.5)"),
            Some(Rgba::new(1, 2, 3, 128))
        );
        assert_eq!(
            parse_color("rgb(1 2 3 / 50%)"),
            Some(Rgba::new(1, 2, 3, 128))
        );
        assert_eq!(
            parse_color("hsl(0, 100%, 50%)"),
            Some(Rgba::new(255, 0, 0, 255))
        );
        assert_eq!(parse_color("Navy"), Some(Rgba::new(0, 0, 128, 255)));
        assert_eq!(
            parse_color("light-dark(#eee,#222)"),
            Some(Rgba::new(0xee, 0xee, 0xee, 255))
        );
        assert_eq!(
            parse_color("light-dark(rgb(1, 2, 3), black)"),
            Some(Rgba::new(1, 2, 3, 255))
        );
        assert_eq!(parse_color("nope"), None);
    }
}
