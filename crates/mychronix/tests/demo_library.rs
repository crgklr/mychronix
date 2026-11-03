//! The made-up library (see `support::demo`): it loads, and can be written
//! out for screenshots with
//! `MYCHRONIX_DEMO_HOME=<dir> cargo test -p mychronix --test demo_library -- --ignored`.

mod support;

#[test]
#[ignore = "writes the made-up library to $MYCHRONIX_DEMO_HOME"]
fn write_demo_library() {
    let home = std::env::var_os("MYCHRONIX_DEMO_HOME").expect("MYCHRONIX_DEMO_HOME");
    support::demo::write_library(std::path::Path::new(&home));
}

#[test]
fn the_track_is_a_closed_loop_of_about_820_m() {
    let line = support::demo::center_line();
    let (a, b) = (line[0], line[line.len() - 1]);
    assert!((a.0 - b.0).hypot(a.1 - b.1) < 1.5, "the loop closes");
    assert!((780..860).contains(&line.len()), "{} m", line.len());
}
