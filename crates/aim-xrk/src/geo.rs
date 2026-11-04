//! Positions near each other as flat meters east and north of an origin. At
//! a kart track's size (a kilometer or so) the earth's curve doesn't matter.

/// Meters per degree of latitude.
pub const M_PER_DEG_LAT: f64 = 110_540.0;

/// Meters per degree of longitude at latitude `lat`.
pub fn m_per_deg_lon(lat: f64) -> f64 {
    111_320.0 * lat.to_radians().cos()
}

/// Meters east and north of `(lat0, lon0)`.
pub fn to_local(lat0: f64, lon0: f64, lat: f64, lon: f64) -> (f64, f64) {
    (
        (lon - lon0) * m_per_deg_lon(lat0),
        (lat - lat0) * M_PER_DEG_LAT,
    )
}

/// The position `(x, y)` meters east and north of `(lat0, lon0)`, as (lat, lon).
pub fn from_local(lat0: f64, lon0: f64, (x, y): (f64, f64)) -> (f64, f64) {
    (lat0 + y / M_PER_DEG_LAT, lon0 + x / m_per_deg_lon(lat0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters_and_positions_round_trip() {
        let (x, y) = to_local(44.52, -89.57, 44.521, -89.571);
        assert!((y - 110.54).abs() < 1e-6 && (x + 79.4).abs() < 0.1);
        let (lat, lon) = from_local(44.52, -89.57, (x, y));
        assert!((lat - 44.521).abs() < 1e-12 && (lon + 89.571).abs() < 1e-12);
    }
}
