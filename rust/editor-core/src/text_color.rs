use std::collections::HashMap;

use cssparser::{Parser, ParserInput};
use serde_json::Value;

pub(crate) const TEXT_STYLE_MARK: &str = "textStyle";
pub(crate) const COLOR_ATTR: &str = "color";
const CHANNEL_MAX: f32 = u8::MAX as f32;

pub(crate) fn canonical_color(value: &str) -> Option<String> {
    let value = value.trim();
    let (red, green, blue, alpha) = if let Some(hex) = value.strip_prefix('#') {
        cssparser::color::parse_hash_color(hex.as_bytes()).ok()?
    } else if value.eq_ignore_ascii_case("transparent") {
        (0, 0, 0, 0.0)
    } else if let Ok((red, green, blue)) = cssparser::color::parse_named_color(value) {
        (red, green, blue, 1.0)
    } else {
        parse_rgb(value)?
    };
    let alpha = (alpha.clamp(0.0, 1.0) * CHANNEL_MAX).round() as u8;
    Some(format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}"))
}

fn parse_rgb(value: &str) -> Option<(u8, u8, u8, f32)> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let function = parser.expect_function().ok()?;
    if !function.eq_ignore_ascii_case("rgb") && !function.eq_ignore_ascii_case("rgba") {
        return None;
    }
    let color = parser
        .parse_nested_block(|parser| {
            let red = parser.expect_number()?;
            let commas = parser.try_parse(|parser| parser.expect_comma()).is_ok();
            let green = parser.expect_number()?;
            if commas {
                parser.expect_comma()?;
            }
            let blue = parser.expect_number()?;
            let alpha = if parser.is_exhausted() {
                1.0
            } else {
                if commas {
                    parser.expect_comma()?;
                } else {
                    parser.expect_delim('/')?;
                }
                parser.expect_number()?
            };
            parser.expect_exhausted()?;
            if ![red, green, blue, alpha]
                .iter()
                .all(|value| value.is_finite())
            {
                return Err(parser.new_custom_error::<(), ()>(()));
            }
            let channel = |value: f32| value.clamp(0.0, CHANNEL_MAX).round() as u8;
            Ok((channel(red), channel(green), channel(blue), alpha))
        })
        .ok()?;
    parser.expect_exhausted().ok()?;
    Some(color)
}

pub(crate) fn valid_color_attrs(mark_type: &str, attrs: &HashMap<String, Value>) -> bool {
    mark_type != TEXT_STYLE_MARK
        || attrs.get(COLOR_ATTR).is_none_or(|color| {
            color.is_null() || color.as_str().and_then(canonical_color).is_some()
        })
}

pub(crate) fn canonicalize_color_attrs(mark_type: &str, attrs: &mut HashMap<String, Value>) {
    if mark_type != TEXT_STYLE_MARK {
        return;
    }
    if let Some(color) = attrs
        .get(COLOR_ATTR)
        .and_then(Value::as_str)
        .and_then(canonical_color)
    {
        attrs.insert(COLOR_ATTR.into(), Value::String(color));
    }
}

pub(crate) fn color_from_style(style: &str) -> Option<String> {
    style
        .split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case(COLOR_ATTR)
                .then(|| canonical_color(value))
                .flatten()
        })
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_color_all_css_names_and_numeric_syntax() {
        for (name, (red, green, blue)) in cssparser::color::all_named_colors() {
            assert_eq!(
                canonical_color(name),
                Some(format!("#{red:02x}{green:02x}{blue:02x}ff")),
                "{name}"
            );
        }
        for (value, expected) in [
            ("#1234", "#11223344"),
            ("#12345678", "#12345678"),
            ("rgb(12 34 56 / .5)", "#0c223880"),
            ("RGB(12, 34, 56)", "#0c2238ff"),
            ("rgb(300 -10 5.5)", "#ff0006ff"),
        ] {
            assert_eq!(canonical_color(value).as_deref(), Some(expected), "{value}");
        }
        for value in [
            "",
            "#12",
            "#ggg",
            "currentColor",
            "var(--x)",
            "rgb(1, 2 3)",
            "rgb(1 2 3) junk",
            "rgb(NaN 2 3)",
            "red;background:blue",
        ] {
            assert_eq!(canonical_color(value), None, "{value}");
        }
    }
}
