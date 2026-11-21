//! Image preparation: EXIF-rotate, flatten alpha, crop-to-fill, rotate, and
//! re-order into the printer's column-planar RGB layout.

use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};
use std::path::Path;

/// Bytes per `SendImage` packet.
pub const SEGMENT_LEN: usize = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    Sp2,
    Sp3,
}

impl Model {
    pub fn name(self) -> &'static str {
        match self {
            Model::Sp2 => "SP-2",
            Model::Sp3 => "SP-3",
        }
    }

    /// Size (w, h) the photo is cropped/resized to before rotation.
    pub fn fit_size(self) -> (u32, u32) {
        match self {
            Model::Sp2 => (600, 800),
            Model::Sp3 => (800, 800),
        }
    }

    /// Size (w, h) of the image that is actually encoded (after a 90° clockwise rotation).
    pub fn print_size(self) -> (usize, usize) {
        match self {
            Model::Sp2 => (800, 600),
            Model::Sp3 => (800, 800),
        }
    }

    pub fn image_len(self) -> usize {
        let (w, h) = self.print_size();
        w * h * 3
    }

    pub fn segments(self) -> usize {
        self.image_len() / SEGMENT_LEN
    }
}

/// Load a photo and fit it to the model's print format (portrait for SP-2, square for SP-3).
pub fn load_and_convert(path: &Path, model: Model) -> Result<RgbImage> {
    let mut decoder = ImageReader::open(path)
        .with_context(|| format!("cannot open {}", path.display()))?
        .with_guessed_format()?
        .into_decoder()
        .with_context(|| format!("cannot decode {}", path.display()))?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);

    // Composite onto white so transparent areas print white, not black.
    let rgba = img.to_rgba8();
    let mut flat = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        flat.put_pixel(x, y, image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])]));
    }

    let (fw, fh) = model.fit_size();
    let fitted = DynamicImage::ImageRgb8(flat).resize_to_fill(fw, fh, FilterType::CatmullRom);
    Ok(fitted.to_rgb8())
}

/// Rotate 90° clockwise and encode into the printer's byte layout.
pub fn encode(fitted: &RgbImage, model: Model) -> Result<Vec<u8>> {
    let rotated = DynamicImage::ImageRgb8(fitted.clone()).rotate90().to_rgb8();
    let (w, h) = model.print_size();
    anyhow::ensure!(
        rotated.width() as usize == w && rotated.height() as usize == h,
        "image is {}x{} after rotation, expected {w}x{h}",
        rotated.width(),
        rotated.height()
    );
    Ok(encode_columns(rotated.as_raw(), w, h))
}

/// Row-major RGB8 (`w` x `h`) -> for each column x: all R values top-to-bottom,
/// then all G, then all B. Output length is `w * h * 3`.
pub fn encode_columns(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    assert_eq!(rgb.len(), w * h * 3);
    let mut out = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let src = (row * w + col) * 3;
            let base = col * h * 3;
            out[base + row] = rgb[src];
            out[base + h + row] = rgb[src + 1];
            out[base + 2 * h + row] = rgb[src + 2];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn column_layout_matches_python() {
        // 4 wide x 3 high; vectors produced by InstaxImage.encodeImage().
        let pix = hex("000102030405060708090a0b28292a2b2c2d2e2f30313233505152535455565758595a5b");
        let expected = hex("002850012951022a52032b53042c54052d55062e56072f570830580931590a325a0b335b");
        assert_eq!(encode_columns(&pix, 4, 3), expected);
    }

    #[test]
    fn model_sizes() {
        assert_eq!(Model::Sp2.image_len(), 1_440_000);
        assert_eq!(Model::Sp3.image_len(), 1_920_000);
        assert_eq!(Model::Sp2.segments(), 24);
        assert_eq!(Model::Sp3.segments(), 32);
    }
}
