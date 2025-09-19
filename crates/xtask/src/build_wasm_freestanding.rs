use std::{fs, path::PathBuf, process::Command};

use anyhow::{anyhow, Result};

use crate::{bail_on_err, BuildWasm};

pub fn run_wasm_freestanding(args: &BuildWasm) -> Result<()> {
    // Use clang directly for freestanding WASM
    let clang = if cfg!(windows) { "clang.exe" } else { "clang" };

    // Check if clang is available
    if Command::new(clang).arg("--version").output().is_err() {
        return Err(anyhow!("clang not found. Please install LLVM/clang"));
    }

    // Read the exports list
    let exports = fs::read_to_string("lib/binding_web/lib/exports.txt")?
        .lines()
        .map(|line| format!("-Wl,--export={}", line.trim_matches('"')))
        .collect::<Vec<String>>();

    // Clean up old filewasm_stors
    for file in [
        "web-tree-sitter.wasm",
        "web-tree-sitter.wasm.map",
        "web-tree-sitter.js",
        "web-tree-sitter.mjs",
    ] {
        fs::remove_file(PathBuf::from("lib/binding_web").join(file)).ok();
    }

    let mut clang_flags = vec![
        "--target=wasm32-unknown-unknown", // Target truly freestanding WebAssembly (no WASI)
        "--no-standard-libraries",      // No libc or any runtime
        "-nostdlib",                    // No standard library
        "-fno-builtin",                 // No builtin functions
        "-ffreestanding",               // Freestanding environment
        "-fvisibility=hidden",          // Hide symbols by default
        "-fno-zero-initialized-in-bss", // Avoid unsupported flags
        "-fno-common",                  // Avoid common symbols
        "-D_POSIX_C_SOURCE=200112L",
        "-D_DEFAULT_SOURCE=",
        "-DNDEBUG=",
        "-Ilib/src",
        "-Ilib/include",
        "-Icrates/language/wasm/include", // Use existing WASM sysroot
        "-Wl,--no-entry",                 // No main function needed
        "-Wl,--import-memory",            // Import memory from JS
        "-Wl,--import-table",             // Import function table
        "-Wl,--export-all",               // Export all functions initially
        "-Wl,--allow-undefined",          // Allow undefined symbols (imported from JS)
        "-fuse-ld=lld",                   // Use LLD linker (includes wasm support)
        "-v",                             // Verbose output to see what's happening
        "-o",
        "lib/binding_web/web-tree-sitter.wasm",
        "lib/src/lib.c",
        "crates/language/wasm/src/stdlib.c", // WASM stdlib implementation
        "crates/language/wasm/src/stdio.c",  // WASM stdio implementation
        "crates/language/wasm/src/string.c", // WASM string implementation
    ];

    // Clear problematic environment variables that might inject unsupported flags
    let mut cmd = Command::new(clang);
    cmd.env_remove("NIX_LDFLAGS")
       .env_remove("NIX_CFLAGS_COMPILE")
       .env_remove("NIX_CXXSTDLIB_COMPILE")
       .env_remove("NIX_CXXSTDLIB_LINK")
       .env_remove("NIX_CC_WRAPPER_TARGET_HOST")
       .env_remove("NIX_HARDENING_ENABLE");

    if args.debug {
        clang_flags.extend(["-O0", "-g"]);
    } else {
        clang_flags.extend([
            "-O3",               // Maximum optimization
            "-flto",             // Link-time optimization
            "-Wl,--lto-O3",      // LTO optimization
            "-Wl,--strip-debug", // Strip debug info
        ]);
    }

    let output = cmd.args(&clang_flags).args(&exports).output()?;

    bail_on_err(&output, "Failed to compile freestanding WASM")?;

    println!(
        "✓ Compiled freestanding web-tree-sitter.wasm ({} bytes)",
        fs::metadata("lib/binding_web/web-tree-sitter.wasm")?.len()
    );

    Ok(())
}
