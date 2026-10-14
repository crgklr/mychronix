//! Runs the sensor check (`aim_xrk::checks`) over session files.
//! `cargo run -p aim-xrk --example dead_channels -- <files…>`
fn main() {
    for path in std::env::args().skip(1) {
        let Ok(s) = aim_xrk::Session::open(std::path::Path::new(&path)) else { continue };
        let issues = aim_xrk::checks::sensor_issues(&s);
        let list: Vec<String> = issues.iter().map(|i| format!("{} {:?}", i.channel, i.kind)).collect();
        println!("{:<20} {}", path.rsplit('/').next().unwrap().chars().take(20).collect::<String>(), if list.is_empty() { "ok".into() } else { list.join(", ") });
    }
}
