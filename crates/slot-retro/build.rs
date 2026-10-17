fn main() {
    println!("cargo:rerun-if-changed=csrc/log.c");
    cc::Build::new()
        .file("csrc/log.c")
        .compile("slot_retro_log");
}
