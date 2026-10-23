//! Checks a MyChron 6 on USB: its serial, the sessions folder, and
//! optionally one file downloaded to the current folder.
//!
//! `cargo run -p aim-link --example usb_probe [-- a_0035.xrz]`

use aim_link::usb::UsbLogger;

fn main() {
    let started = std::time::Instant::now();
    let mut logger = UsbLogger::open().expect("open");
    println!("serial: {:?}", logger.logger_id().expect("serial"));
    let entries = logger.list(aim_link::SESSIONS_DIR).expect("list");
    println!("{} files in {}", entries.len(), aim_link::SESSIONS_DIR);
    if let Some(name) = std::env::args().nth(1) {
        let path = format!("{}/{name}", aim_link::SESSIONS_DIR);
        let data = logger.read_file(&path, |_, _| {}).expect("read");
        std::fs::write(&name, &data).expect("save");
        println!("saved {name}: {} bytes", data.len());
    }
    println!("took {:?}", started.elapsed());
}
