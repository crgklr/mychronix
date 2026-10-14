//! Prints each file's logger id and model.
fn main() {
    for p in std::env::args().skip(1) {
        if let Ok(s) = aim_xrk::Session::open(std::path::Path::new(&p)) {
            println!("{:<40} {:?} {:?} {:?}", p.rsplit('/').next().unwrap(), s.info.logger_id, s.info.logger_model, s.info.logger_model_id);
        }
    }
}
