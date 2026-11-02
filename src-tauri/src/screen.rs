use base64::{engine::general_purpose::STANDARD, Engine};
use image::{codecs::jpeg::JpegEncoder, imageops, RgbImage, RgbaImage};
use xcap::Monitor;

/// Geometry of the monitor a frame was taken from, in the coordinate space the input
/// layer uses. Model coordinates (0..1000) are mapped through this.
#[derive(Clone, Copy, Debug)]
pub struct Geom {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Size of the (downscaled) screenshot the model actually saw.
    pub iw: u32,
    pub ih: u32,
}

impl Geom {
    pub fn map(&self, nx: f64, ny: f64) -> (i32, i32) {
        let px = self.x + (nx.clamp(0.0, 1000.0) / 1000.0 * (self.w - 1) as f64) as i32;
        let py = self.y + (ny.clamp(0.0, 1000.0) / 1000.0 * (self.h - 1) as f64) as i32;
        (px, py)
    }
}

pub fn capture(around: (i32, i32), max_w: u32) -> Result<(String, Geom), String> {
    let mon = match Monitor::from_point(around.0, around.1) {
        Ok(m) => m,
        Err(_) => Monitor::all()
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or("no monitor")?,
    };
    let geom = Geom {
        x: mon.x().map_err(|e| e.to_string())?,
        y: mon.y().map_err(|e| e.to_string())?,
        w: mon.width().map_err(|e| e.to_string())? as i32,
        h: mon.height().map_err(|e| e.to_string())? as i32,
        iw: 0,
        ih: 0,
    };
    let shot = mon.capture_image().map_err(|e| e.to_string())?;
    let (w, h) = (shot.width(), shot.height());
    let rgba = RgbaImage::from_raw(w, h, shot.into_raw()).ok_or("bad frame")?;
    let target_w = w.min(max_w.max(256));
    let target_h = (h as f64 * target_w as f64 / w as f64) as u32;
    let small = imageops::resize(&rgba, target_w, target_h, imageops::FilterType::Triangle);
    let rgb: RgbImage = image::DynamicImage::ImageRgba8(small).to_rgb8();
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 60)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok((STANDARD.encode(jpeg), Geom { iw: target_w, ih: target_h, ..geom }))
}
