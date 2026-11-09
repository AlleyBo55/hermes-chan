fn main() {
    // Her sprites live in overlay/src/hermeschan, inside frontendDist, so Tauri embeds them
    // like the rest of her page; nothing is copied in at build time.
    tauri_build::build();
}
