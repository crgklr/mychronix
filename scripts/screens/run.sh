#!/bin/sh
# Runs the screenshot tests (crates/mychronix/tests/screens.rs) as CI does:
# Ubuntu 22.04 in a container, on a virtual display with Mesa's software
# renderer. `--update` rewrites the reference images in
# crates/mychronix/tests/screens/ (review them before committing).
# Uses podman, or docker with CONTAINER=docker.
set -e
cd "$(dirname "$0")/../.."
run=${CONTAINER:-podman}
$run image inspect mychronix-screens >/dev/null 2>&1 ||
    $run build -t mychronix-screens -f scripts/screens/Containerfile scripts/screens
cache="$HOME/.cache/mychronix-screens"
mkdir -p "$cache/target" "$HOME/.cargo/registry" "$HOME/.cargo/git"
update=
[ "$1" = --update ] && update=1
$run run --rm --init \
    -v "$PWD":/src:z -v "$cache/target":/target:z \
    -v "$HOME/.cargo/registry":/root/.cargo/registry:z -v "$HOME/.cargo/git":/root/.cargo/git:z \
    -e CARGO_TARGET_DIR=/target -e MYCHRONIX_SCREEN_TESTS=1 ${update:+-e MYCHRONIX_SCREEN_UPDATE=1} \
    -w /src mychronix-screens \
    xvfb-run -a -s "-screen 0 2560x1600x24" cargo test --locked -p mychronix --test screens -- --nocapture ||
    { echo "Screenshots and differences: $cache/target/tmp/screens/"; exit 1; }
