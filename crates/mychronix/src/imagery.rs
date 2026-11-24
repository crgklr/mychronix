//! Aerial photos from national mapping agencies, each used inside its own
//! borders: sharper there than the worldwide satellite imagery, which is used
//! everywhere else. The borders come from `assets/imagery-areas.json`
//! (`scripts/imagery-areas.py`).

use std::collections::HashMap;
use std::sync::LazyLock;

/// One agency's aerial photos, as web map tiles.
pub struct Source {
    /// The area it covers: an ISO 3166-1 or 3166-2 code in the borders file.
    pub area: &'static str,
    /// The tile address, with `{z}`, `{x}` and `{y}`; `{z-5:02}` for a
    /// service that counts zooms from 5, two digits; or, for a map service
    /// that draws any rectangle, `{bbox}` (the tile's corners in web
    /// Mercator meters: west,south,east,north).
    pub url: &'static str,
    /// The closest zoom with photos; closer, they're shown enlarged.
    pub max_zoom: u8,
    /// Whose photos they are, as the agency asks to be credited.
    pub credit: &'static str,
}

impl Source {
    pub fn tile_url(&self, z: u8, x: u32, y: u32) -> String {
        let mut url = self
            .url
            .replace("{z}", &z.to_string())
            .replace("{x}", &x.to_string())
            .replace("{y}", &y.to_string());
        if url.contains("{bbox}") {
            let (west, north) = mercator(x, y, z);
            let (east, south) = mercator(x + 1, y + 1, z);
            url = url.replace(
                "{bbox}",
                &format!("{west:.2},{south:.2},{east:.2},{north:.2}"),
            );
        }
        // "{z-5:02}": the zoom less 5, two digits.
        if let Some(start) = url.find("{z-")
            && let Some(len) = url[start..].find('}')
        {
            let spec = &url[start + 3..start + len];
            let (less, width) = spec.split_once(':').unwrap_or((spec, "0"));
            let less: u8 = less.parse().unwrap_or(0);
            let width: usize = width.parse().unwrap_or(0);
            let value = format!("{:0width$}", z.saturating_sub(less));
            url.replace_range(start..start + len + 1, &value);
        }
        url
    }
}

/// The national sources, checked to work without a key and to be free to use
/// with credit.
pub const SOURCES: &[Source] = &[
    Source {
        area: "CH",
        url: "https://wmts.geo.admin.ch/1.0.0/ch.swisstopo.swissimage/default/current/3857/{z}/{x}/{y}.jpeg",
        max_zoom: 20,
        credit: "swisstopo",
    },
    Source {
        area: "NL",
        url: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/Actueel_orthoHR/EPSG:3857/{z}/{x}/{y}.jpeg",
        max_zoom: 20,
        credit: "Beeldmateriaal.nl (PDOK)",
    },
    Source {
        area: "ES",
        url: "https://www.ign.es/wmts/pnoa-ma?request=GetTile&service=WMTS&VERSION=1.0.0&Layer=OI.OrthoimageCoverage&Style=default&Format=image/jpeg&TileMatrixSet=GoogleMapsCompatible&TileMatrix={z}&TileRow={y}&TileCol={x}",
        max_zoom: 20,
        credit: "PNOA © Instituto Geográfico Nacional de España",
    },
    Source {
        area: "FR",
        url: "https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=ORTHOIMAGERY.ORTHOPHOTOS&STYLE=normal&FORMAT=image/jpeg&TILEMATRIXSET=PM&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}",
        max_zoom: 19,
        credit: "IGN-F / Géoportail",
    },
    Source {
        area: "LU",
        url: "https://wmts1.geoportail.lu/opendata/wmts/ortho_latest/GLOBAL_WEBMERCATOR_4_V3/{z}/{x}/{y}.jpeg",
        max_zoom: 20,
        credit: "ACT Luxembourg",
    },
    Source {
        area: "AT",
        url: "https://mapsneu.wien.gv.at/basemap/bmaporthofoto30cm/normal/google3857/{z}/{y}/{x}.jpeg",
        max_zoom: 20,
        credit: "basemap.at",
    },
    Source {
        area: "BE-BRU",
        url: "https://geoservices-grid.irisnet.be/geowebcache/service/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=Ortho&STYLE=&TILEMATRIXSET=EPSG:900913&TILEMATRIX=EPSG:900913:{z}&TILEROW={y}&TILECOL={x}&FORMAT=image/jpeg",
        max_zoom: 21,
        credit: "UrbIS",
    },
    Source {
        area: "BE-VLG",
        url: "https://geo.api.vlaanderen.be/OMWRGBMRVL/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=omwrgbmrvl&STYLE=&TILEMATRIXSET=GoogleMapsVL&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}&FORMAT=image/png",
        max_zoom: 21,
        credit: "Digitaal Vlaanderen",
    },
    Source {
        area: "BE-WAL",
        url: "https://geoservices.wallonie.be/arcgis/rest/services/IMAGERIE/ORTHO_LAST/MapServer/export?bbox={bbox}&bboxSR=3857&imageSR=3857&size=256,256&format=jpg&f=image",
        max_zoom: 20,
        credit: "SPW",
    },
    Source {
        area: "DE-NW",
        url: "https://www.wmts.nrw.de/geobasis/wmts_nw_dop/tiles/nw_dop/EPSG_3857_16/{z-5:02}/{x}/{y}",
        max_zoom: 20,
        credit: "GeoBasis-DE / NRW",
    },
    Source {
        area: "DE-BY",
        url: "https://wmtsod1.bayernwolke.de/wmts/by_dop/smerc/{z}/{x}/{y}",
        max_zoom: 19,
        credit: "Datenquellen: Bayerische Vermessungsverwaltung, Europäische Union, enthält Copernicus Sentinel-2 Daten 2018, verarbeitet durch das Bundesamt für Kartographie und Geodäsie (BKG)",
    },
    Source {
        area: "DE-BW",
        url: "https://owsproxy.lgl-bw.de/owsproxy/ows/WMS_LGL-BW_ATKIS_DOP_20_C?LAYERS=IMAGES_DOP_20_RGB&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "LGL-BW, dl-de/by-2.0",
    },
    Source {
        area: "DE-HE",
        url: "https://www.gds-srv.hessen.de/cgi-bin/lika-services/ogc-free-images.ows?LAYERS=he_dop20_rgb&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "HVBG Hessen",
    },
    Source {
        area: "DE-NI",
        url: "https://opendata.lgln.niedersachsen.de/doorman/noauth/dop_wms?LAYERS=ni_dop20&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "LGLN, CC BY 4.0",
    },
    Source {
        area: "DE-SN",
        url: "https://geodienste.sachsen.de/wms_geosn_dop-rgb/guest?LAYERS=sn_dop_020&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "GeoSN, dl-de/by-2.0",
    },
    Source {
        area: "DE-TH",
        url: "https://www.geoproxy.geoportal-th.de/geoproxy/services/DOP?LAYERS=th_dop&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "GDI-Th",
    },
    Source {
        area: "DE-BB",
        url: "https://isk.geobasis-bb.de/mapproxy/dop20c/service/wms?LAYERS=bebb_dop20c&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "GeoBasis-DE/LGB, dl-de/by-2-0",
    },
    Source {
        area: "DE-BE",
        url: "https://gdi.berlin.de/services/wms/truedop_2025_sommer?LAYERS=truedop_2025_sommer_rgb&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "Geoportal Berlin",
    },
    Source {
        area: "DE-RP",
        url: "https://geo4.service24.rlp.de/wms/rp_dop20.fcgi?LAYERS=rp_dop20&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "GeoBasis-DE / LVermGeoRP, dl-de/by-2-0",
    },
    Source {
        area: "DE-SH",
        url: "https://dienste.gdi-sh.de/WMS_SH_DOP20col_OpenGBD?LAYERS=sh_dop20_rgb&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "GeoBasis-DE/LVermGeo SH/CC BY 4.0",
    },
    Source {
        area: "DE-HB",
        url: "https://geodienste.bremen.de/wms_dop_lb?LAYERS=dop10_2025_HB&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&STYLES=&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "Landesamt GeoInformation Bremen",
    },
    Source {
        area: "CA-QC",
        url: "https://servicesmatriciels.mern.gouv.qc.ca/erdas-iws/ogc/wmts/Imagerie_Continue/Imagerie_GQ/default/GoogleMapsCompatibleExt2:epsg:3857/{z}/{y}/{x}.jpg",
        max_zoom: 20,
        credit: "Gouvernement du Québec",
    },
    Source {
        area: "AU-QLD",
        url: "https://spatial-img.information.qld.gov.au/arcgis/rest/services/Basemaps/LatestStateProgram_AllUsers/ImageServer/tile/{z}/{y}/{x}",
        max_zoom: 20,
        credit: "State of Queensland; Planet Labs Netherlands B.V., reproduced under licence from Planet and Geoplex",
    },
    Source {
        area: "CZ",
        url: "https://ags.cuzk.gov.cz/arcgis1/rest/services/ORTOFOTO_WM/MapServer/tile/{z}/{y}/{x}",
        max_zoom: 20,
        credit: "ČÚZK",
    },
    Source {
        area: "PL",
        url: "https://mapy.geoportal.gov.pl/wss/service/PZGIK/ORTO/WMTS/StandardResolution?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=ORTOFOTOMAPA&STYLE=default&FORMAT=image/jpeg&TILEMATRIXSET=EPSG:3857&TILEMATRIX=EPSG:3857:{z}&TILEROW={y}&TILECOL={x}",
        max_zoom: 19,
        credit: "GUGiK, geoportal.gov.pl",
    },
    Source {
        area: "EE",
        url: "https://tiles.maaamet.ee/tm/wmts/1.0.0/foto/default/GMC/{z}/{y}/{x}.jpg",
        max_zoom: 18,
        credit: "Republic of Estonia Land and Spatial Development Board",
    },
    Source {
        area: "IT-BZ",
        url: "https://geoservices.buergernetz.bz.it/mapproxy/root/wmts/p_bz-Orthoimagery:Aerial-2023-RGB/EPSG_3857/{z}/{x}/{y}.png",
        max_zoom: 20,
        credit: "Provincia autonoma di Bolzano – Alto Adige",
    },
    Source {
        area: "SI",
        url: "https://ipi.eprostor.gov.si/wms-si-gurs-dts/wms?SERVICE=WMS&VERSION=1.1.1&REQUEST=GetMap&LAYERS=SI.GURS.ZPDZ:DOF025&STYLES=&SRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&FORMAT=image/jpeg",
        max_zoom: 20,
        credit: "GURS",
    },
    Source {
        area: "PT",
        url: "https://ortos.dgterritorio.gov.pt/wms/ortosat2023?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ortoSat2023-CorVerdadeira&STYLES=&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&FORMAT=image/jpeg",
        max_zoom: 19,
        credit: "DGT",
    },
];

/// Below this zoom a tile spans tens of kilometers: the worldwide imagery.
const MIN_ZOOM: u8 = 12;

/// An area's outline: its rings (lon, lat) and their bounding box.
struct Area {
    rings: Vec<Vec<(f64, f64)>>,
    min: (f64, f64),
    max: (f64, f64),
}

impl Area {
    fn contains(&self, lon: f64, lat: f64) -> bool {
        lon >= self.min.0
            && lon <= self.max.0
            && lat >= self.min.1
            && lat <= self.max.1
            && self.rings.iter().any(|r| in_ring(r, lon, lat))
    }
}

/// Even-odd ray casting.
fn in_ring(ring: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let ((xi, yi), (xj, yj)) = (ring[i], ring[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

static AREAS: LazyLock<HashMap<String, Area>> = LazyLock::new(|| {
    let raw: HashMap<String, Vec<Vec<[f64; 2]>>> =
        serde_json::from_str(include_str!("../assets/imagery-areas.json"))
            .expect("bundled imagery areas are valid");
    raw.into_iter()
        .map(|(code, rings)| {
            let rings: Vec<Vec<(f64, f64)>> = rings
                .into_iter()
                .map(|r| r.into_iter().map(|[x, y]| (x, y)).collect())
                .collect();
            let points = rings.iter().flatten();
            let min = points
                .clone()
                .fold((f64::MAX, f64::MAX), |m, p| (m.0.min(p.0), m.1.min(p.1)));
            let max = points.fold((f64::MIN, f64::MIN), |m, p| (m.0.max(p.0), m.1.max(p.1)));
            (code, Area { rings, min, max })
        })
        .collect()
});

/// A web map tile's corner `(x, y)` at `zoom` in web Mercator meters.
fn mercator(x: u32, y: u32, zoom: u8) -> (f64, f64) {
    const HALF: f64 = 20_037_508.342_789_244;
    let n = f64::from(1u32 << zoom);
    (
        f64::from(x) / n * 2.0 * HALF - HALF,
        HALF - f64::from(y) / n * 2.0 * HALF,
    )
}

/// (lon, lat) of a web map tile's corner `(x, y)` at `zoom`.
fn corner(x: u32, y: u32, zoom: u8) -> (f64, f64) {
    let n = f64::from(1u32 << zoom);
    let lon = f64::from(x) / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * f64::from(y) / n))
        .sinh()
        .atan()
        .to_degrees();
    (lon, lat)
}

/// The national source for a tile: one whose area holds the whole tile.
pub fn source_for(x: u32, y: u32, zoom: u8) -> Option<&'static Source> {
    if !(MIN_ZOOM..=22).contains(&zoom) {
        return None;
    }
    let corners = [
        corner(x, y, zoom),
        corner(x + 1, y, zoom),
        corner(x, y + 1, zoom),
        corner(x + 1, y + 1, zoom),
    ];
    SOURCES.iter().find(|s| {
        AREAS
            .get(s.area)
            .is_some_and(|a| corners.iter().all(|&(lon, lat)| a.contains(lon, lat)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tile at `zoom` holding (lat, lon).
    fn tile(lat: f64, lon: f64, zoom: u8) -> (u32, u32) {
        let n = f64::from(1u32 << zoom);
        let x = ((lon + 180.0) / 360.0 * n) as u32;
        let y = ((1.0 - lat.to_radians().tan().asinh() / std::f64::consts::PI) / 2.0 * n) as u32;
        (x, y)
    }

    fn area_at(lat: f64, lon: f64, zoom: u8) -> Option<&'static str> {
        let (x, y) = tile(lat, lon, zoom);
        source_for(x, y, zoom).map(|s| s.area)
    }

    #[test]
    fn addresses_fill_in_the_tile() {
        let source = |url| Source {
            area: "",
            url,
            max_zoom: 20,
            credit: "",
        };
        assert_eq!(source("t/{z}/{x}/{y}").tile_url(18, 5, 7), "t/18/5/7");
        assert_eq!(source("t/{z-5:02}/{x}/{y}").tile_url(9, 1, 2), "t/04/1/2");
        assert_eq!(source("t/{z-5:02}/{x}/{y}").tile_url(21, 1, 2), "t/16/1/2");
        // Zoom 1, tile (1, 0): the north-east quarter of the world.
        assert_eq!(
            source("w?BBOX={bbox}").tile_url(1, 1, 0),
            "w?BBOX=0.00,0.00,20037508.34,20037508.34"
        );
    }

    #[test]
    fn every_source_has_an_area() {
        for s in SOURCES {
            assert!(AREAS.contains_key(s.area), "{}", s.area);
            let tile = (s.url.contains("{z}") || s.url.contains("{z-"))
                && s.url.contains("{x}")
                && s.url.contains("{y}");
            assert!(tile || s.url.contains("{bbox}"), "{}", s.area);
        }
    }

    #[test]
    fn a_tile_inside_a_country_uses_its_photos() {
        assert_eq!(area_at(47.37, 8.54, 18), Some("CH")); // Zurich
        assert_eq!(area_at(52.37, 4.89, 18), Some("NL")); // Amsterdam
        assert_eq!(area_at(40.42, -3.70, 18), Some("ES")); // Madrid
        assert_eq!(area_at(48.86, 2.35, 18), Some("FR")); // Paris
        // States and regions with their own photos.
        assert_eq!(area_at(51.23, 6.77, 18), Some("DE-NW")); // Düsseldorf
        assert_eq!(area_at(48.78, 9.18, 18), Some("DE-BW")); // Stuttgart
        assert_eq!(area_at(50.85, 4.35, 18), Some("BE-BRU")); // Brussels
        assert_eq!(area_at(51.05, 3.72, 18), Some("BE-VLG")); // Ghent
        assert_eq!(area_at(50.63, 5.58, 18), Some("BE-WAL")); // Liège
        assert_eq!(area_at(45.50, -73.57, 18), Some("CA-QC")); // Montreal
        assert_eq!(area_at(-27.47, 153.03, 18), Some("AU-QLD")); // Brisbane
        assert_eq!(area_at(50.09, 14.42, 18), Some("CZ")); // Prague
        assert_eq!(area_at(46.50, 11.35, 18), Some("IT-BZ")); // Bolzano
    }

    #[test]
    fn elsewhere_and_far_out_the_worldwide_imagery() {
        assert_eq!(area_at(41.06, -73.54, 18), None); // Connecticut
        assert_eq!(area_at(30.0, -40.0, 18), None); // the Atlantic
        assert_eq!(area_at(47.37, 8.54, 8), None); // too far out
    }
}
