use std::{env, fs, path::PathBuf, process::Command, time::SystemTime};

fn main() {
    if let Some(git_sha) = read_git_sha() {
        println!("cargo:rustc-env=BUILD_SHA={git_sha}");
    }

    copy_playground_files();

    println!("cargo:rustc-check-cfg=cfg(sanitizing)");

    let build_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    println!("cargo:rustc-env=BUILD_TIME={build_time}");

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
    ))]
    {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap()).join("dynamic-symbols.txt");
        std::fs::write(
            &out_dir,
            "{
                ts_current_malloc;
                ts_current_calloc;
                ts_current_realloc;
                ts_current_free;
            };",
        )
        .unwrap();
        println!(
            "cargo:rustc-link-arg=-Wl,--dynamic-list={}",
            out_dir.display()
        );
    }
}

fn copy_playground_files() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets_dir = manifest_dir.join("assets");
    fs::create_dir_all(&assets_dir).unwrap();

    let files = [
        ("../docs/src/assets/js/playground.js", "playground.js"),
        ("../lib/binding_web/tree-sitter.js", "tree-sitter.js"),
        ("../lib/binding_web/tree-sitter.wasm", "tree-sitter.wasm"),
    ];

    // Copy files if they exist
    for (src, dest) in files {
        let src_path = manifest_dir.join(src);
        let dest_path = assets_dir.join(dest);

        if src_path.exists() {
            println!("cargo:rerun-if-changed={}", src_path.display());
            fs::copy(&src_path, &dest_path).unwrap_or_else(|e| {
                panic!(
                    "Failed to copy {} to {}: {e}",
                    src_path.display(),
                    dest_path.display(),
                );
            });
        } else {
            // During package publication, files should already be in assets/
            if !dest_path.exists() {
                println!("cargo:warning=Playground file not found: {src}. Package may not work correctly.");
            }
        }
    }
}

// When updating this function, don't forget to also update generate/build.rs which has a
// near-identical function.
fn read_git_sha() -> Option<String> {
    let crate_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    if !crate_path.parent().is_some_and(|p| p.join(".git").exists()) {
        return None;
    }

    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(crate_path)
        .output()
        .map_or(None, |output| {
            if !output.status.success() {
                return None;
            }
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        })
}
