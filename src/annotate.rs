//! Annotation shapes and rendering them onto the final image.
use ab_glyph::{FontArc, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_circle_mut, draw_polygon_mut, draw_text_mut};
use imageproc::point::Point;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Arrow,
    Rect,
    Text,
}

/// All coordinates are in image pixels.
#[derive(Clone, Debug)]
pub enum Shape {
    Arrow { a: [f32; 2], b: [f32; 2], color: [u8; 4], width: f32 },
    Rect { a: [f32; 2], b: [f32; 2], color: [u8; 4], width: f32 },
    Text { pos: [f32; 2], text: String, color: [u8; 4], size: f32 },
}

pub const FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial Bold.ttf",
];

pub fn load_font_bytes() -> Option<Vec<u8>> {
    FONT_CANDIDATES.iter().find_map(|p| std::fs::read(p).ok())
}

/// Arrow geometry shared by preview and export: (shaft_end, head_left, head_right).
pub fn arrow_head(a: [f32; 2], b: [f32; 2], width: f32) -> Option<([f32; 2], [f32; 2], [f32; 2])> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head_len = (width * 4.0).max(12.0).min(len);
    let half = (width * 2.5).max(7.0);
    let base = [b[0] - ux * head_len, b[1] - uy * head_len];
    let (nx, ny) = (-uy * half, ux * half);
    Some((base, [base[0] + nx, base[1] + ny], [base[0] - nx, base[1] - ny]))
}

fn pt(p: [f32; 2]) -> Point<i32> {
    Point::new(p[0].round() as i32, p[1].round() as i32)
}

fn polygon(img: &mut RgbaImage, pts: &[[f32; 2]], color: Rgba<u8>) {
    let mut v: Vec<Point<i32>> = pts.iter().map(|p| pt(*p)).collect();
    v.dedup();
    while v.len() > 1 && v.first() == v.last() {
        v.pop();
    }
    if v.len() >= 3 {
        draw_polygon_mut(img, &v, color);
    }
}

fn thick_line(img: &mut RgbaImage, a: [f32; 2], b: [f32; 2], w: f32, color: Rgba<u8>) {
    let w = w.max(2.0);
    let r = (w / 2.0).round().max(1.0) as i32;
    draw_filled_circle_mut(img, (a[0].round() as i32, a[1].round() as i32), r, color);
    draw_filled_circle_mut(img, (b[0].round() as i32, b[1].round() as i32), r, color);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return;
    }
    let (nx, ny) = (-dy / len * w / 2.0, dx / len * w / 2.0);
    polygon(
        img,
        &[
            [a[0] + nx, a[1] + ny],
            [b[0] + nx, b[1] + ny],
            [b[0] - nx, b[1] - ny],
            [a[0] - nx, a[1] - ny],
        ],
        color,
    );
}

pub fn render(base: &RgbaImage, shapes: &[Shape], font: Option<&FontArc>) -> RgbaImage {
    let mut img = base.clone();
    for s in shapes {
        match s {
            Shape::Arrow { a, b, color, width } => {
                let c = Rgba(*color);
                if let Some((shaft_end, l, r)) = arrow_head(*a, *b, *width) {
                    thick_line(&mut img, *a, shaft_end, *width, c);
                    polygon(&mut img, &[*b, l, r], c);
                }
            }
            Shape::Rect { a, b, color, width } => {
                let c = Rgba(*color);
                let (x0, y0, x1, y1) = (a[0], a[1], b[0], b[1]);
                thick_line(&mut img, [x0, y0], [x1, y0], *width, c);
                thick_line(&mut img, [x1, y0], [x1, y1], *width, c);
                thick_line(&mut img, [x1, y1], [x0, y1], *width, c);
                thick_line(&mut img, [x0, y1], [x0, y0], *width, c);
            }
            Shape::Text { pos, text, color, size } => {
                if let Some(f) = font {
                    for (i, line) in text.lines().enumerate() {
                        draw_text_mut(
                            &mut img,
                            Rgba(*color),
                            pos[0].round() as i32,
                            (pos[1] + i as f32 * size * 1.2).round() as i32,
                            PxScale::from(*size),
                            f,
                            line,
                        );
                    }
                }
            }
        }
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_without_panic() {
        let base = RgbaImage::from_pixel(200, 100, Rgba([255, 255, 255, 255]));
        let font = load_font_bytes().and_then(|b| FontArc::try_from_vec(b).ok());
        let c = [255, 0, 0, 255];
        let shapes = vec![
            Shape::Arrow { a: [10.0, 10.0], b: [150.0, 80.0], color: c, width: 6.0 },
            Shape::Arrow { a: [5.0, 5.0], b: [5.0, 5.0], color: c, width: 1.0 },
            Shape::Arrow { a: [5.0, 5.0], b: [7.0, 5.0], color: c, width: 30.0 },
            Shape::Rect { a: [20.0, 20.0], b: [20.0, 20.0], color: c, width: 1.0 },
            Shape::Rect { a: [0.0, 0.0], b: [200.0, 100.0], color: c, width: 8.0 },
            Shape::Text { pos: [30.0, 30.0], text: "Hello\nworld".into(), color: c, size: 24.0 },
        ];
        let out = render(&base, &shapes, font.as_ref());
        assert_eq!(out.dimensions(), (200, 100));
        assert_ne!(out.get_pixel(80, 45), base.get_pixel(80, 45));
    }
}
