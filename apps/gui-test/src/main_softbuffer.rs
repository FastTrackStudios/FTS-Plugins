//! Standalone GUI test — softbuffer rendering path (for comparison).
//!
//! Same test component as main.rs but using the softbuffer-blit feature.
//! Compare this with gui-test to see the difference.
//!
//! Run with:
//!   nix develop --command cargo run -p gui-test --bin gui-test-softbuffer

fn main() {
    eprintln!("To test the softbuffer path, build with:");
    eprintln!("  cargo run -p gui-test --bin gui-test-softbuffer --features softbuffer-blit");
    eprintln!();
    eprintln!("This binary exists as a placeholder. The actual rendering path is");
    eprintln!("selected at compile time via the softbuffer-blit feature flag.");
    eprintln!();
    eprintln!("Use 'gui-test' (without --bin) to test the native wgpu surface path.");
}
