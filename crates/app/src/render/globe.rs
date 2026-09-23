//! Original wireframe globe for the network panel (ADR-006).
//!
//! Orthographic projection of public-domain Natural Earth coastlines (see
//! `docs/provenance.csv`) with markers for connection peers located by the
//! offline GeoIP database. Rotation is driven by the app at 20 Hz (≤ 30 Hz ceiling) only
//! while the globe is visible and motion is allowed; otherwise it is static.

use std::f32::consts::PI;
use std::sync::OnceLock;

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Cache, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer};

use super::cells::{mix, to_color};
use crate::config::Theme;

const COASTLINE_ASSET: &[u8] = include_bytes!("../../../../assets/geo/coastline-110m.bin");
/// Degrees per second when rotating.
pub const ROTATION_SPEED: f32 = 6.0;
/// Most markers drawn; further peers are counted but not plotted.
pub const MAX_MARKERS: usize = 64;
/// View tilt so both hemispheres are visible.
const TILT_DEGREES: f32 = 18.0;

/// Coastline polylines as (longitude, latitude) in radians.
pub struct Coastlines {
    pub lines: Vec<Vec<(f32, f32)>>,
}

/// Parse the bundled asset (format documented in `scripts/convert_coastline.py`).
pub fn parse_coastlines(bytes: &[u8]) -> Option<Coastlines> {
    let mut reader = Reader { bytes, at: 0 };
    let count = reader.u16()?;
    let mut lines = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let points = reader.u16()?;
        let mut line = Vec::with_capacity(usize::from(points));
        for _ in 0..points {
            let lon = f32::from(reader.i16()?) / 100.0;
            let lat = f32::from(reader.i16()?) / 100.0;
            if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
                return None;
            }
            line.push((lon.to_radians(), lat.to_radians()));
        }
        lines.push(line);
    }
    (reader.at == bytes.len()).then_some(Coastlines { lines })
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let slice = self.bytes.get(self.at..self.at + N)?;
        self.at += N;
        slice.try_into().ok()
    }
    fn u16(&mut self) -> Option<u16> {
        self.take::<2>().map(u16::from_le_bytes)
    }
    fn i16(&mut self) -> Option<i16> {
        self.take::<2>().map(i16::from_le_bytes)
    }
}

/// The bundled coastlines, parsed once. Empty if the asset were ever corrupt.
pub fn coastlines() -> &'static Coastlines {
    static CACHE: OnceLock<Coastlines> = OnceLock::new();
    CACHE.get_or_init(|| {
        parse_coastlines(COASTLINE_ASSET).unwrap_or(Coastlines { lines: Vec::new() })
    })
}

/// Orthographic projection centred on (`lon0`, `lat0`), all in radians.
/// Returns unit-disc coordinates (y up) and whether the point faces the viewer.
pub fn project(lon0: f32, lat0: f32, lon: f32, lat: f32) -> (f32, f32, bool) {
    let dlon = lon - lon0;
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lat0, cos_lat0) = lat0.sin_cos();
    let cos_c = sin_lat0 * sin_lat + cos_lat0 * cos_lat * dlon.cos();
    let x = cos_lat * dlon.sin();
    let y = cos_lat0 * sin_lat - sin_lat0 * cos_lat * dlon.cos();
    // Small tolerance keeps points exactly on the limb visible despite rounding.
    (x, y, cos_c >= -1e-6)
}

/// A located peer, in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    pub latitude: f32,
    pub longitude: f32,
}

/// Rotation state owned by the app; the cache is cleared on each tick.
pub struct GlobeState {
    pub rotation_degrees: f32,
    pub cache: Cache,
}

impl Default for GlobeState {
    fn default() -> Self {
        Self {
            rotation_degrees: 0.0,
            cache: Cache::new(),
        }
    }
}

impl GlobeState {
    /// Advance rotation by `seconds` of elapsed time (clamped to avoid jumps after pauses).
    pub fn advance(&mut self, seconds: f32) {
        let step = seconds.clamp(0.0, 0.1) * ROTATION_SPEED;
        self.rotation_degrees = (self.rotation_degrees + step).rem_euclid(360.0);
        self.cache.clear();
    }

    /// Face the first marker when motion is off, so the static view is useful.
    pub fn face(&mut self, marker: Option<Marker>) {
        let target = marker.map_or(0.0, |m| m.longitude).rem_euclid(360.0);
        if (target - self.rotation_degrees).abs() > f32::EPSILON {
            self.rotation_degrees = target;
            self.cache.clear();
        }
    }
}

pub struct GlobeView<'a> {
    pub state: &'a GlobeState,
    pub markers: &'a [Marker],
    pub theme: &'a Theme,
}

impl<Message> canvas::Program<Message> for GlobeView<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        vec![
            self.state
                .cache
                .draw(renderer, bounds.size(), |frame| self.paint(frame)),
        ]
    }
}

impl GlobeView<'_> {
    fn paint(&self, frame: &mut Frame) {
        let size = frame.size();
        let radius = (size.width.min(size.height) / 2.0 - 4.0).max(1.0);
        let center = Point::new(size.width / 2.0, size.height / 2.0);
        let accent = to_color(self.theme.colors.accent);
        let surface = to_color(self.theme.colors.surface);
        let lon0 = self.state.rotation_degrees.to_radians();
        let lat0 = TILT_DEGREES.to_radians();
        let to_screen =
            |(x, y): (f32, f32)| Point::new(center.x + radius * x, center.y - radius * y);

        frame.fill(
            &Path::circle(center, radius),
            mix(surface, Color::BLACK, 0.35),
        );
        let view = ViewRotation::new(lon0, lat0);
        let (coast_lines, graticule_lines) = unit_layers();
        let faint = Stroke::default()
            .with_color(Color { a: 0.18, ..accent })
            .with_width(0.6);
        stroke_visible(frame, graticule_lines, &view, &to_screen, faint);
        let coast = Stroke::default()
            .with_color(Color { a: 0.85, ..accent })
            .with_width(1.0);
        stroke_visible(frame, coast_lines, &view, &to_screen, coast);
        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default().with_color(accent).with_width(1.2),
        );
        self.paint_markers(frame, lon0, lat0, &to_screen);
    }

    fn paint_markers(
        &self,
        frame: &mut Frame,
        lon0: f32,
        lat0: f32,
        to_screen: &impl Fn((f32, f32)) -> Point,
    ) {
        let hot = to_color(self.theme.colors.error);
        for marker in self.markers.iter().take(MAX_MARKERS) {
            let (x, y, visible) = project(
                lon0,
                lat0,
                marker.longitude.to_radians(),
                marker.latitude.to_radians(),
            );
            if !visible {
                continue;
            }
            let at = to_screen((x, y));
            frame.fill(&Path::circle(at, 5.0), Color { a: 0.25, ..hot });
            frame.fill(&Path::circle(at, 2.2), hot);
        }
    }
}

/// Stroke the front-facing runs of many polylines as ONE path, so the frame
/// is tessellated once per layer instead of once per line.
fn stroke_visible(
    frame: &mut Frame,
    lines: &[Vec<[f32; 3]>],
    view: &ViewRotation,
    to_screen: &impl Fn((f32, f32)) -> Point,
    stroke: Stroke<'_>,
) {
    let path = Path::new(|builder| {
        for line in lines {
            let mut pen_down = false;
            for &point in line {
                let (x, y, visible) = view.project(point);
                if !visible {
                    pen_down = false;
                    continue;
                }
                let point = to_screen((x, y));
                if pen_down {
                    builder.line_to(point);
                } else {
                    builder.move_to(point);
                    pen_down = true;
                }
            }
        }
    });
    frame.stroke(&path, stroke);
}

/// Per-point precomputation: `(cos φ·cos λ, cos φ·sin λ, sin φ)`.
fn unit(lon: f32, lat: f32) -> [f32; 3] {
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();
    [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat]
}

/// Per-frame trig, so projecting each point is multiply-add only.
struct ViewRotation {
    cos_lon0: f32,
    sin_lon0: f32,
    cos_lat0: f32,
    sin_lat0: f32,
}

impl ViewRotation {
    fn new(lon0: f32, lat0: f32) -> Self {
        let (sin_lon0, cos_lon0) = lon0.sin_cos();
        let (sin_lat0, cos_lat0) = lat0.sin_cos();
        Self {
            cos_lon0,
            sin_lon0,
            cos_lat0,
            sin_lat0,
        }
    }

    /// Same result as [`project`] for a precomputed unit vector.
    fn project(&self, [a, b, sin_lat]: [f32; 3]) -> (f32, f32, bool) {
        let cos_dlon_term = a * self.cos_lon0 + b * self.sin_lon0;
        let x = b * self.cos_lon0 - a * self.sin_lon0;
        let y = self.cos_lat0 * sin_lat - self.sin_lat0 * cos_dlon_term;
        let depth = self.sin_lat0 * sin_lat + self.cos_lat0 * cos_dlon_term;
        (x, y, depth >= -1e-6)
    }
}

/// Polylines of precomputed unit vectors.
type UnitLines = Vec<Vec<[f32; 3]>>;

/// Coastlines and graticule as precomputed unit vectors, built once.
fn unit_layers() -> &'static (UnitLines, UnitLines) {
    static LAYERS: OnceLock<(UnitLines, UnitLines)> = OnceLock::new();
    LAYERS.get_or_init(|| {
        let convert = |lines: &[Vec<(f32, f32)>]| -> UnitLines {
            lines
                .iter()
                .map(|line| line.iter().map(|&(lon, lat)| unit(lon, lat)).collect())
                .collect()
        };
        (convert(&coastlines().lines), convert(&graticule()))
    })
}

/// Meridians every 30° and parallels every 30°, sampled every 5°.
fn graticule() -> Vec<Vec<(f32, f32)>> {
    let deg = |d: i32| d as f32 * PI / 180.0;
    let meridians = (-180..180).step_by(30).map(|lon| {
        (-90..=90)
            .step_by(5)
            .map(|lat| (deg(lon), deg(lat)))
            .collect()
    });
    let parallels = (-60..=60).step_by(30).map(|lat| {
        (-180..=180)
            .step_by(5)
            .map(|lon| (deg(lon), deg(lat)))
            .collect()
    });
    meridians.chain(parallels).collect()
}

/// Deduplicate located peers (≈1° grid) and cap the number drawn.
pub fn markers_from(locations: impl Iterator<Item = (f64, f64)>) -> Vec<Marker> {
    let mut seen = std::collections::HashSet::new();
    locations
        .filter(|(lat, lon)| {
            lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
        })
        .filter(|(lat, lon)| seen.insert((lat.round() as i32, lon.round() as i32)))
        .take(MAX_MARKERS)
        .map(|(lat, lon)| Marker {
            latitude: lat as f32,
            longitude: lon as f32,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_coastlines_parse_completely() {
        let coast = parse_coastlines(COASTLINE_ASSET).expect("asset parses");
        assert_eq!(coast.lines.len(), 134);
        assert_eq!(coast.lines.iter().map(Vec::len).sum::<usize>(), 5128);
    }

    #[test]
    fn truncated_or_out_of_range_assets_are_rejected() {
        assert!(parse_coastlines(&COASTLINE_ASSET[..COASTLINE_ASSET.len() - 1]).is_none());
        assert!(parse_coastlines(&[]).is_none());
        let bad = [1, 0, 1, 0, 0x51, 0x46, 0, 0]; // one point at lon 180.01°: out of range
        assert!(parse_coastlines(&bad).is_none());
    }

    #[test]
    fn precomputed_projection_matches_reference() {
        let view = ViewRotation::new(1.1, 0.3);
        for (lon, lat) in [(0.0, 0.0), (2.0, -0.7), (-2.9, 1.2), (0.5, 0.5)] {
            let (x1, y1, v1) = project(1.1, 0.3, lon, lat);
            let (x2, y2, v2) = view.project(unit(lon, lat));
            assert!((x1 - x2).abs() < 1e-5 && (y1 - y2).abs() < 1e-5 && v1 == v2);
        }
    }

    #[test]
    fn projection_centre_and_far_side() {
        let (x, y, visible) = project(0.0, 0.0, 0.0, 0.0);
        assert!(visible && x.abs() < 1e-6 && y.abs() < 1e-6);
        let (_, _, back) = project(0.0, 0.0, PI, 0.0);
        assert!(!back);
        let (x, _, edge) = project(0.0, 0.0, PI / 2.0, 0.0);
        assert!(edge && (x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rotation_advances_and_wraps() {
        let mut globe = GlobeState {
            rotation_degrees: 359.8,
            ..GlobeState::default()
        };
        globe.advance(1.0); // clamped to 0.1 s
        // 1 s is clamped to 0.1 s → +0.6°, wrapping past 360.
        assert!((globe.rotation_degrees - 0.4).abs() < 1e-3);
        globe.face(Some(Marker {
            latitude: 10.0,
            longitude: -75.0,
        }));
        assert_eq!(globe.rotation_degrees, 285.0);
    }

    #[test]
    fn markers_are_deduplicated_validated_and_capped() {
        let points = vec![
            (40.7, -74.0),
            (40.71, -74.01),
            (f64::NAN, 0.0),
            (95.0, 0.0),
            (51.5, -0.1),
        ];
        let markers = markers_from(points.into_iter());
        assert_eq!(markers.len(), 2);
        let many = (0..200).map(|i| (f64::from(i % 90), f64::from(i) - 100.0));
        assert!(markers_from(many).len() <= MAX_MARKERS);
    }
}
