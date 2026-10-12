//! `cargo run -p aim-track --example inspect -- <file.ria|file.tkk|dir>...`
//! A directory is swept: every .tkk in it is parsed and failures listed.
fn main() {
    for arg in std::env::args().skip(1) {
        let path = std::path::Path::new(&arg);
        if path.is_dir() {
            let (mut ok, mut bad, mut cities) = (0, 0, 0);
            for e in std::fs::read_dir(path).unwrap().flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "tkk") {
                    match aim_track::read_tkk(&p) {
                        Ok(t) => {
                            ok += 1;
                            cities += t.info.city.is_some() as usize;
                        }
                        Err(err) => {
                            bad += 1;
                            println!("FAILED {}: {err}", p.display());
                        }
                    }
                }
            }
            println!("{arg}: {ok} parsed, {bad} failed, {cities} with a city");
        } else if arg.ends_with(".tkk") {
            match aim_track::read_tkk(&arg) {
                Ok(t) => println!(
                    "{arg}: {:?} ({:?}) id {} {} {}/{} {:?} m, {}, {} outline points, tz {:?}, logo {} bytes\n    {:?}",
                    t.name, t.full_name, t.id, t.guid.as_deref().unwrap_or("-"), t.region, t.country, t.length_m,
                    t.kind.label(), t.outline.len(), t.timezone, t.logo_png.as_ref().map_or(0, Vec::len), t.info.fields
                ),
                Err(e) => println!("{arg}: {e}"),
            }
        } else {
            match aim_track::read_ria(&arg) {
                Ok(t) => println!("{arg}: {} tracks; first {:?} {:?} m", t.len(), t.first().map(|r| &r.name), t.first().and_then(|r| r.length_m)),
                Err(e) => println!("{arg}: {e}"),
            }
        }
    }
}
