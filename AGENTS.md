# Agent rules

- After every successful `cargo build`, `cargo test`, `cargo nextest` or `cargo run` in this repo, run `tools/target-cap/target-cap.sh`. It runs `cargo clean` only when `target/` is over 10 GB, so it is cheap to run every time. Skip it if the build failed.
