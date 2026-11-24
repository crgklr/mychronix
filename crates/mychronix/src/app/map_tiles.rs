//! The tiles under the maps (Tracks, Map, the start/finish line picker):
//! OpenStreetMap, or satellite imagery: a national agency's aerial photos
//! where there are some (see `crate::imagery`), Esri's elsewhere.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui;

use walkers::Tiles;

use crate::imagery;

/// `tiles` for a map, or none with MYCHRONIX_NO_TILES (see `hooks`).
pub(super) fn shown(tiles: &mut MapTiles) -> Option<&mut dyn walkers::Tiles> {
    (!crate::hooks::get().no_tiles).then_some(tiles as &mut dyn walkers::Tiles)
}

/// The map's tiles, OpenStreetMap or satellite, made on first use (shared by Tracks and Map).
pub(super) fn tiles<'a>(
    street: &'a mut Option<MapTiles>,
    imagery: &'a mut Option<MapTiles>,
    ctx: &egui::Context,
    satellite: bool,
) -> &'a mut MapTiles {
    if satellite {
        imagery.get_or_insert_with(|| MapTiles {
            http: walkers::HttpTiles::with_options(EsriImagery, options(), ctx.clone()),
            fallbacks: &SATELLITE_FALLBACKS,
            national: Some(National {
                ctx: ctx.clone(),
                tiles: HashMap::new(),
                asked: HashMap::new(),
            }),
            credits: Vec::new(),
            credit: "Esri, Maxar, Earthstar Geographics",
            prefix: "Imagery ©",
        })
    } else {
        street.get_or_insert_with(|| MapTiles {
            http: walkers::HttpTiles::with_options(
                walkers::sources::OpenStreetMap,
                options(),
                ctx.clone(),
            ),
            fallbacks: &[],
            national: None,
            credits: Vec::new(),
            credit: "OpenStreetMap contributors",
            prefix: "Map data ©",
        })
    }
}

fn options() -> walkers::HttpOptions {
    walkers::HttpOptions {
        cache: crate::paths::cache_dir().map(|d| d.join("tiles")),
        user_agent: Some(walkers::HeaderValue::from_static(
            "Mychronix/0.1 (AiM MyChron data analysis; Linux desktop)",
        )),
        ..Default::default()
    }
}

/// Zooms whose satellite tiles are fetched beside closer ones: where there's
/// no closer imagery, the map shows these enlarged (zoom 17 is about a meter
/// a pixel, and nearly everywhere has it).
const SATELLITE_FALLBACKS: [u8; 2] = [17, 15];

/// How long a national tile may take before Esri's is shown instead: the
/// agency's service may be down.
const NATIONAL_WAIT: Duration = Duration::from_secs(3);

/// A map's tiles, which also fetch the same place at `fallbacks` zooms: a
/// tile that doesn't exist up close is drawn from the closest of those.
pub(super) struct MapTiles {
    http: walkers::HttpTiles,
    fallbacks: &'static [u8],
    /// Satellite only: the national agencies' photos.
    national: Option<National>,
    /// Whose tiles were drawn since the credit line was last asked for.
    credits: Vec<&'static str>,
    /// Whose `http` tiles are, and how a credit line starts.
    credit: &'static str,
    prefix: &'static str,
}

/// Each national source's tiles, made the first time a map shows its area,
/// and when each tile was first asked for.
struct National {
    ctx: egui::Context,
    tiles: HashMap<&'static str, walkers::HttpTiles>,
    asked: HashMap<walkers::TileId, Instant>,
}

impl MapTiles {
    /// The credit for what was drawn since the last call: "Imagery ©
    /// swisstopo, Esri, …", nationals first.
    pub(super) fn credit(&mut self) -> String {
        let mut whose = std::mem::take(&mut self.credits);
        if whose.is_empty() {
            whose.push(self.credit);
        }
        whose.sort_by_key(|c| (*c == self.credit, *c));
        whose.dedup();
        format!("{} {}", self.prefix, whose.join(", "))
    }

    fn national_at(&mut self, id: walkers::TileId) -> Option<Option<walkers::TilePiece>> {
        let national = self.national.as_mut()?;
        let source = imagery::source_for(id.x, id.y, id.zoom)?;
        let http = national.tiles.entry(source.area).or_insert_with(|| {
            walkers::HttpTiles::with_options(Agency(source), options(), national.ctx.clone())
        });
        fetch_fallbacks(http, self.fallbacks, id);
        if national.asked.len() > 20_000 {
            national.asked.clear();
        }
        let asked = *national.asked.entry(id).or_insert_with(Instant::now);
        match http.at(id) {
            Some(piece) => {
                self.credits.push(source.credit);
                Some(Some(piece))
            }
            // Still coming: nothing yet (or the closest zoom it has).
            None if asked.elapsed() < NATIONAL_WAIT => Some(None),
            // Not coming: Esri's.
            None => None,
        }
    }
}

/// Asks `http` for the tiles at `fallbacks` zooms holding `id`.
fn fetch_fallbacks(http: &mut walkers::HttpTiles, fallbacks: &[u8], id: walkers::TileId) {
    for &zoom in fallbacks.iter().filter(|&&z| z < id.zoom) {
        let d = id.zoom - zoom;
        http.at(walkers::TileId {
            x: id.x >> d,
            y: id.y >> d,
            zoom,
        });
    }
}

impl Tiles for MapTiles {
    fn at(&mut self, id: walkers::TileId) -> Option<walkers::TilePiece> {
        if let Some(piece) = self.national_at(id) {
            return piece;
        }
        fetch_fallbacks(&mut self.http, self.fallbacks, id);
        let piece = self.http.at(id);
        if piece.is_some() {
            self.credits.push(self.credit);
        }
        piece
    }

    fn attribution(&self) -> walkers::sources::Attribution {
        self.http.attribution()
    }

    fn tile_size(&self) -> u32 {
        self.http.tile_size()
    }
}

/// A national agency's aerial photos.
struct Agency(&'static imagery::Source);

impl walkers::sources::TileSource for Agency {
    fn tile_url(&self, id: walkers::TileId) -> String {
        self.0.tile_url(id.zoom, id.x, id.y)
    }

    fn attribution(&self) -> walkers::sources::Attribution {
        walkers::sources::Attribution {
            text: self.0.credit,
            url: "",
            logo_light: None,
            logo_dark: None,
        }
    }

    fn max_zoom(&self) -> u8 {
        self.0.max_zoom
    }
}

/// Esri World Imagery: satellite and aerial photos. Free for personal,
/// non-commercial use with attribution.
struct EsriImagery;

impl walkers::sources::TileSource for EsriImagery {
    fn tile_url(&self, id: walkers::TileId) -> String {
        format!(
            // Where there's no imagery this close, "not found" (not Esri's
            // "Map data not yet available" picture): `MapTiles` shows a
            // farther one enlarged.
            "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}?blankTile=false",
            id.zoom, id.y, id.x
        )
    }

    fn attribution(&self) -> walkers::sources::Attribution {
        walkers::sources::Attribution {
            text: "Esri, Maxar, Earthstar Geographics",
            url: "https://www.esri.com/",
            logo_light: None,
            logo_dark: None,
        }
    }
}
