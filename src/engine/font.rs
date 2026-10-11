//! Embedded DejaVu Sans Bold coverage masks; no runtime font library.
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct Font {
    pub(super) glyphs: Arc<HashMap<char, Glyph>>,
    pub(super) size: f32,
}
pub(super) struct Glyph {
    pub advance: f32,
    pub x: f32,
    pub y: f32,
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<u8>,
}
impl Font {
    pub fn embedded() -> Self {
        Self::from_bytes(include_bytes!("../../assets/font.dwf")).expect("invalid embedded font")
    }
    pub fn from_bytes(data: &[u8]) -> Result<Self, &'static str> {
        let mut p = 0;
        fn take<'a>(data: &'a [u8], p: &mut usize, n: usize) -> Result<&'a [u8], &'static str> {
            let part = data
                .get(*p..p.checked_add(n).ok_or("font length overflow")?)
                .ok_or("truncated font")?;
            *p += n;
            Ok(part)
        }
        if take(data, &mut p, 4)? != b"DWFT" {
            return Err("invalid font signature");
        }
        let size = u16::from_le_bytes(take(data, &mut p, 2)?.try_into().unwrap());
        if size == 0 {
            return Err("invalid font size");
        }
        let count = u16::from_le_bytes(take(data, &mut p, 2)?.try_into().unwrap());
        let mut glyphs = HashMap::new();
        for _ in 0..count {
            let record = take(data, &mut p, 20)?;
            let char = char::from_u32(u32::from_le_bytes(record[0..4].try_into().unwrap()))
                .ok_or("invalid glyph")?;
            let advance = f32::from_le_bytes(record[4..8].try_into().unwrap());
            if !advance.is_finite() || advance < 0.0 {
                return Err("invalid advance");
            }
            let x = i16::from_le_bytes(record[8..10].try_into().unwrap()) as f32;
            let y = i16::from_le_bytes(record[10..12].try_into().unwrap()) as f32;
            let w = u16::from_le_bytes(record[12..14].try_into().unwrap()) as usize;
            let h = u16::from_le_bytes(record[14..16].try_into().unwrap()) as usize;
            let n = u32::from_le_bytes(record[16..20].try_into().unwrap()) as usize;
            if w * h > 1_048_576 || n % 3 != 0 {
                return Err("invalid glyph dimensions");
            }
            let mut pixels = Vec::with_capacity(w * h);
            for run in take(data, &mut p, n)?.chunks_exact(3) {
                let len = u16::from_le_bytes([run[0], run[1]]) as usize;
                if len == 0 || pixels.len() + len > w * h {
                    return Err("invalid glyph coverage");
                }
                pixels.resize(pixels.len() + len, run[2]);
            }
            if pixels.len() != w * h {
                return Err("invalid glyph coverage");
            }
            glyphs.insert(
                char,
                Glyph {
                    advance,
                    x,
                    y,
                    w,
                    h,
                    pixels,
                },
            );
        }
        if p != data.len() || !glyphs.contains_key(&'\u{fffd}') {
            return Err("incomplete font");
        }
        Ok(Self {
            glyphs: Arc::new(glyphs),
            size: size as f32,
        })
    }
    pub(super) fn glyph(&self, char: char) -> &Glyph {
        self.glyphs.get(&char).unwrap_or(&self.glyphs[&'\u{fffd}'])
    }
}
pub struct TextParams<'a> {
    pub font: Option<&'a Font>,
    pub font_size: u16,
    pub font_scale: f32,
    pub color: super::Color,
}
impl Default for TextParams<'_> {
    fn default() -> Self {
        Self {
            font: None,
            font_size: 16,
            font_scale: 1.0,
            color: super::WHITE,
        }
    }
}
pub struct TextDimensions {
    pub width: f32,
}
pub fn measure_text(text: &str, font: Option<&Font>, size: u16, scale: f32) -> TextDimensions {
    let fallback;
    let font = match font {
        Some(f) => f,
        None => {
            fallback = Font::embedded();
            &fallback
        }
    };
    TextDimensions {
        width: text.chars().map(|c| font.glyph(c).advance).sum::<f32>() * size as f32 * scale
            / font.size,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn font_retains_ui_glyphs_and_baseline() {
        let f = Font::embedded();
        for c in "Dice Wars — → … ✓ 12345".chars() {
            assert!(f.glyphs.contains_key(&c));
        }
        assert!(f.glyph('A').y < 0.0);
        assert!(
            measure_text("WWW", Some(&f), 24, 1.0).width
                > measure_text("iii", Some(&f), 24, 1.0).width
        );
    }
    #[test]
    fn malformed_fonts_are_rejected() {
        assert!(Font::from_bytes(b"DWFT\x80\x00\x01\x00").is_err());
    }
}
