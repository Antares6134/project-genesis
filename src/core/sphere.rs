//! The planet's geometry — the *horizontal* counterpart of the vertical datum.
//!
//! [`crate::core::relief`] gives every stage the same metres; this module gives
//! it the same kilometres. That matters because the engine's canvas is an
//! equirectangular 8K plate, which is famously **not** uniform in area: a pixel
//! on the 60th parallel covers half the ground of one on the equator. Anything
//! that reports an area, a population or a population *density* therefore has to
//! integrate over latitude instead of counting pixels, exactly as the relief
//! report integrates over the vertical datum instead of trusting heightmap units.
//!
//! The sphere is the one the whole engine already assumes when it builds its
//! seamless 3D noise: a sphere of radius [`PLANET_RADIUS_KM`], sampled by an
//! equirectangular canvas of arbitrary resolution. Every function here is
//! resolution-agnostic, so a 2K test world measures the same continents as the
//! 8K production plate.

/// Radius of the generated planet in kilometres.
pub const PLANET_RADIUS_KM: f32 = 6371.0;

/// Latitude in radians of canvas row `y` on a canvas `height` rows tall.
///
/// Row 0 is the north pole (+90°), the last row the south pole (−90°), and the
/// canvas is centred on the equator — the same convention the terrain, climate
/// and hydrology stages all evaluate their spherical noise with.
#[inline(always)]
pub fn row_latitude_rad(y: f32, height: usize) -> f32 {
    let span = (height.saturating_sub(1)).max(1) as f32;
    (0.5 - y / span) * std::f32::consts::PI
}

/// Latitude in degrees (negative = southern hemisphere) of canvas row `y`.
#[inline(always)]
pub fn row_latitude_deg(y: f32, height: usize) -> f32 {
    row_latitude_rad(y, height).to_degrees()
}

/// Ground area of one map pixel at row `y`, in km², on a `width × height` canvas.
///
/// The longitudinal width of a pixel is constant while its meridional width
/// shrinks with the cosine of latitude, so the exact ground area of a cell of an
/// equirectangular grid is `R² · Δlat · Δlon · cos(lat)`.
#[inline(always)]
pub fn pixel_area_km2(y: usize, height: usize, width: usize) -> f32 {
    let d_lat = std::f32::consts::PI / (height.saturating_sub(1)).max(1) as f32;
    let d_lon = std::f32::consts::TAU / width.max(1) as f32;
    let lat = row_latitude_rad(y as f32, height);
    PLANET_RADIUS_KM * PLANET_RADIUS_KM * d_lat * d_lon * lat.cos()
}

/// Shortest wrap-around distance between two canvas positions, in pixels.
///
/// The map is a cylinder: the left and right edges are the same meridian, so the
/// long way round is never the right answer.
#[inline(always)]
pub fn wrap_distance_px(x1: f32, y1: f32, x2: f32, y2: f32, width: usize) -> f32 {
    let w = width as f32;
    let mut dx = (x1 - x2).abs();
    if dx > w * 0.5 {
        dx = w - dx;
    }
    let dy = y1 - y2;
    (dx * dx + dy * dy).sqrt()
}

/// Great-circle distance between two canvas positions, in kilometres.
///
/// This is the figure the civilization stage reports when it says a realm rose
/// "2 400 km from its nearest neighbour": an honest surface distance on the
/// sphere, not a pixel count that silently lies at high latitude.
pub fn map_distance_km(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    width: usize,
    height: usize,
) -> f32 {
    let w = width as f32;
    let lat1 = row_latitude_rad(y1, height);
    let lat2 = row_latitude_rad(y2, height);
    let lon1 = (x1 / w - 0.5) * std::f32::consts::TAU;
    let lon2 = (x2 / w - 0.5) * std::f32::consts::TAU;

    let d_lat = lat2 - lat1;
    let mut d_lon = (lon2 - lon1).abs();
    if d_lon > std::f32::consts::PI {
        d_lon = std::f32::consts::TAU - d_lon;
    }

    let a = (d_lat * 0.5).sin().powi(2)
        + lat1.cos() * lat2.cos() * (d_lon * 0.5).sin().powi(2);
    2.0 * PLANET_RADIUS_KM * a.sqrt().clamp(-1.0, 1.0).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latitudes_run_pole_to_pole() {
        assert!((row_latitude_deg(0.0, 4096) - 90.0).abs() < 0.05);
        assert!(row_latitude_deg(2047.5, 4096).abs() < 0.05);
        assert!((row_latitude_deg(4095.0, 4096) + 90.0).abs() < 0.05);
    }

    #[test]
    fn pixel_area_shrinks_towards_the_poles() {
        let equatorial = pixel_area_km2(2048, 4096, 8192);
        let subpolar = pixel_area_km2(3072, 4096, 8192); // ≈ 45° N
        // cos(45°) = 0.7071, and an 8K pixel is ≈ 24 km² at the equator.
        assert!((equatorial - 23.9).abs() < 0.5, "equatorial pixel was {equatorial}");
        assert!((subpolar / equatorial - 0.7071).abs() < 0.01);
    }

    #[test]
    fn distances_wrap_around_the_seam() {
        let across = wrap_distance_px(10.0, 100.0, 8182.0, 100.0, 8192);
        let straight = wrap_distance_px(10.0, 100.0, 4100.0, 100.0, 8192);
        assert!(across < straight, "the seam must be the short way round");
        assert!((across - 20.0).abs() < 0.001);
    }

    #[test]
    fn great_circle_distance_matches_a_quarter_circumference() {
        // Equator (row centre) to the north pole is a quarter of a great circle.
        let quarter = std::f32::consts::FRAC_PI_2 * PLANET_RADIUS_KM;
        let measured = map_distance_km(0.0, 2047.5, 0.0, 0.0, 8192, 4096);
        assert!((measured - quarter).abs() < 5.0, "measured {measured}");
    }
}
