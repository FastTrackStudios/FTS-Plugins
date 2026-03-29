use reaper_test::runner::{self, TestPackage, TestRunner};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // If first real arg is "reaper-test", handle it ourselves.
    // Otherwise delegate to nih_plug_xtask (for bundle, etc.).
    if args.get(1).map(|s| s.as_str()) == Some("reaper-test") {
        let package = args
            .get(2)
            .ok_or("Usage: cargo xtask reaper-test <package> [filter] [--keep-open]")?
            .clone();
        let keep_open = args.iter().any(|a| a == "--keep-open");
        let headless = !args.iter().any(|a| a == "--no-headless");
        let filter = args.get(3).filter(|a| !a.starts_with('-')).cloned();
        reaper_test(&package, filter, keep_open, headless)
    } else {
        nih_plug_xtask::main().map_err(|e| e.into())
    }
}

fn reaper_test(
    package: &str,
    filter: Option<String>,
    keep_open: bool,
    headless: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ci = std::env::var("CI").is_ok();
    let timeout_secs: u64 = std::env::var("REAPER_TEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let resources_dir = runner::fts_reaper_resources();

    let runner = TestRunner {
        resources_dir: resources_dir.clone(),
        extension_log: PathBuf::from("/tmp/daw-bridge.log"),
        timeout_secs,
        keep_open,
        headless,
        ci,
        extension_whitelist: Vec::new(),
    };

    // ── Step 1: Ensure daw-bridge extension is installed ─────────────────
    runner::section(ci, "reaper-test: check daw-bridge");
    let user_plugins_dir = resources_dir.join("UserPlugins");
    std::fs::create_dir_all(&user_plugins_dir)?;

    let bridge_installed = user_plugins_dir.join("reaper_daw_bridge.so").exists()
        || user_plugins_dir.join("libreaper_daw_bridge.so").exists()
        || user_plugins_dir.join("reaper_daw_bridge.dylib").exists();

    if bridge_installed {
        println!("  daw-bridge: already installed");
    } else {
        // Try to build from the daw repo if available at ../daw
        let daw_repo = workspace_root.parent().and_then(|p| {
            let daw = p.join("daw");
            daw.exists().then_some(daw)
        });

        if let Some(daw_path) = daw_repo {
            println!("  Building daw-bridge from {} ...", daw_path.display());
            let status = Command::new("cargo")
                .args(["build", "-p", "daw-bridge"])
                .current_dir(&daw_path)
                .status()?;
            if !status.success() {
                return Err("Failed to build daw-bridge from daw repo".into());
            }
            let so_path = daw_path.join("target/debug/libreaper_daw_bridge.so");
            if so_path.exists() {
                runner::install_plugin(&so_path, "reaper_daw_bridge.so", &user_plugins_dir)?;
            } else {
                return Err(format!("daw-bridge built but {} not found", so_path.display()).into());
            }
        } else {
            return Err(
                "daw-bridge extension not installed and daw repo not found at ../daw.\n\
                 Install it with: cd ../daw && cargo build -p daw-bridge\n\
                 Then: ln -s $(pwd)/target/debug/libreaper_daw_bridge.so \
                 ~/.config/FastTrackStudio/Reaper/UserPlugins/reaper_daw_bridge.so"
                    .into(),
            );
        }
    }
    runner::end_section(ci);

    // ── Step 2: Bundle and install the plugin under test ─────────────────
    runner::section(ci, "reaper-test: bundle plugin");
    println!("  Bundling {package}...");
    let status = Command::new("cargo")
        .args([
            "run",
            "--package",
            "xtask",
            "--",
            "bundle",
            package,
            "--release",
        ])
        .current_dir(workspace_root)
        .status()?;
    if !status.success() {
        return Err(format!("Failed to bundle {package}").into());
    }

    // Install the CLAP to FX dir
    let clap_file = format!("{package}.clap");
    let bundled = workspace_root.join("target/bundled").join(&clap_file);
    let fx_dir = user_plugins_dir.join("FX");
    std::fs::create_dir_all(&fx_dir)?;
    if bundled.exists() {
        let dest = fx_dir.join(&clap_file);
        if dest.exists() {
            let _ = std::fs::remove_file(&dest).or_else(|_| std::fs::remove_dir_all(&dest));
        }
        if bundled.is_dir() {
            copy_dir_recursive(&bundled, &dest)?;
        } else {
            std::fs::copy(&bundled, &dest)?;
        }
        println!("  Installed {clap_file} -> {}", fx_dir.display());
    } else {
        println!("  WARNING: {clap_file} not found at {}", bundled.display());
    }
    runner::end_section(ci);

    // ── Step 3: Build test binaries ──────────────────────────────────────
    runner::section(ci, "reaper-test: build test binaries");
    println!("  Building test binaries for {package}...");
    let status = Command::new("cargo")
        .args(["test", "-p", package, "--no-run"])
        .current_dir(workspace_root)
        .status()?;
    if !status.success() {
        return Err(format!("Failed to build test binaries for {package}").into());
    }
    runner::end_section(ci);

    // ── Step 4: Clean sockets, pre-warm, patch INI ───────────────────────
    runner.clean_stale_sockets();
    runner.prewarm_reaper();
    runner.patch_ini();

    // ── Step 5: Spawn REAPER ─────────────────────────────────────────────
    let mut reaper = runner.spawn_reaper()?;
    reaper.wait_for_socket(&runner)?;

    // ── Step 6: Run tests ────────────────────────────────────────────────
    let packages = vec![TestPackage {
        package: package.to_string(),
        features: vec![],
        test_threads: 1,
        default_skips: vec![],
        test_binary: None,
    }];

    let tests_passed = runner.run_tests(&mut reaper, &packages, filter.as_deref())?;

    // ── Step 7: Cleanup ──────────────────────────────────────────────────
    if !tests_passed {
        reaper.report_failure(&runner);
        reaper.stop(&runner);
        return Err("Some tests failed".into());
    }

    reaper.stop(&runner);
    println!("\nAll tests passed!");
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
