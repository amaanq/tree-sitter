use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

const BUILD_VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
    let cli_cargo = Path::new("Cargo.toml");

    let mut version = if let Some(build_sha) = read_git_sha() {
        format!("{:10}{} ({})", "", BUILD_VERSION, build_sha)
    } else {
        BUILD_VERSION.to_string()
    };

    if let Some(git_describe) = read_git_describe() {
        if !git_describe.is_empty() && !git_describe.starts_with(format!("v{BUILD_VERSION}").as_str()) {
            version += format!(", git: {}", git_describe).as_str();
        }
    }

    let rust_binding_version = read_dependency_version(cli_cargo, "tree-sitter");
    let config_version = read_dependency_version(cli_cargo, "tree-sitter-config");
    let loader_version = read_dependency_version(cli_cargo, "tree-sitter-loader");
    let highlight_version = read_dependency_version(cli_cargo, "tree-sitter-highlight");
    let tags_version = read_dependency_version(cli_cargo, "tree-sitter-tags");

    version += format!("; {:21} {config_version}", "tree-sitter-config").as_str();
    version += format!("; {:21} {loader_version}", "tree-sitter-loader").as_str();
    version += format!("; {:21} {highlight_version}", "tree-sitter-highlight").as_str();
    version += format!("; {:21} {tags_version}", "tree-sitter-tags").as_str();

    println!("cargo:rustc-env={}={}", "TREE_SITTER_CLI_VERSION", version);

    println!(
        "cargo:rustc-env={}={}",
        "RUST_BINDING_VERSION", rust_binding_version,
    );

    let emscripten_version = fs::read_to_string("emscripten-version").unwrap();
    println!(
        "cargo:rustc-env={}={}",
        "EMSCRIPTEN_VERSION",
        emscripten_version.trim_end(),
    );

    if web_playground_files_present() {
        println!("cargo:rustc-cfg={}", "TREE_SITTER_EMBED_WASM_BINDING");
    }
}

fn web_playground_files_present() -> bool {
    let paths = [
        "../docs/assets/js/playground.js",
        "../lib/binding_web/tree-sitter.js",
        "../lib/binding_web/tree-sitter.wasm",
    ];

    paths.iter().all(|p| Path::new(p).exists())
}

fn read_git_sha() -> Option<String> {
    let mut repo_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let mut git_path;
    loop {
        git_path = repo_path.join(".git");
        if git_path.exists() {
            break;
        } else if !repo_path.pop() {
            return None;
        }
    }

    let git_dir_path;
    if git_path.is_dir() {
        git_dir_path = git_path;
    } else if let Ok(git_path_content) = fs::read_to_string(&git_path) {
        git_dir_path = repo_path.join(git_path_content.get("gitdir: ".len()..).unwrap().trim_end());
    } else {
        return None;
    }
    let git_head_path = git_dir_path.join("HEAD");
    if let Some(path) = git_head_path.to_str() {
        println!("cargo:rerun-if-changed={}", path);
    }
    if let Ok(mut head_content) = fs::read_to_string(&git_head_path) {
        if head_content.ends_with("\n") {
            head_content.pop();
        }

        // If we're on a branch, read the SHA from the ref file.
        if head_content.starts_with("ref: ") {
            head_content.replace_range(0.."ref: ".len(), "");
            let ref_filename = {
                // Go to real non-worktree gitdir
                let git_dir_path = git_dir_path
                    .parent()
                    .map(|p| {
                        p.file_name()
                            .map(|n| n == OsStr::new("worktrees"))
                            .and_then(|x| x.then(|| p.parent()))
                    })
                    .flatten()
                    .flatten()
                    .unwrap_or(&git_dir_path);

                let file = git_dir_path.join(&head_content);
                if file.is_file() {
                    file
                } else {
                    let packed_refs = git_dir_path.join("packed-refs");
                    if let Ok(packed_refs_content) = fs::read_to_string(&packed_refs) {
                        for line in packed_refs_content.lines() {
                            if let Some((hash, r#ref)) = line.split_once(' ') {
                                if r#ref == head_content {
                                    if let Some(path) = packed_refs.to_str() {
                                        println!("cargo:rerun-if-changed={}", path);
                                    }
                                    return Some(hash.trim_end().to_string());
                                }
                            }
                        }
                    }
                    return None;
                }
            };
            if let Some(path) = ref_filename.to_str() {
                println!("cargo:rerun-if-changed={}", path);
            }
            return fs::read_to_string(&ref_filename)
                .ok()
                .map(|s| s.trim_end().to_string());
        }
        // If we're on a detached commit, then the `HEAD` file itself contains the sha.
        else if head_content.len() == 40 {
            return Some(head_content);
        }
    }

    None
}

fn read_toml_value(toml_path: &Path, path_fn: &(dyn Fn(&toml::Value) -> &toml::Value)) -> String {
    let text = fs::read_to_string(toml_path).unwrap();
    let cargo_toml = toml::from_str::<toml::Value>(text.as_ref()).unwrap();
    path_fn(&cargo_toml)
        .as_str()
        .unwrap()
        .trim_matches('"')
        .to_string()
}

fn read_dependency_version(toml_path: &Path, name: &str) -> String {
    read_toml_value(toml_path, &|v| &v["dependencies"][name]["version"])
}

fn read_git_describe() -> Option<String> {
    Command::new("git")
        .arg("describe")
        .arg("--tags")
        .output()
        .map(|o| {
            String::from_utf8(o.stdout)
                .unwrap_or_default()
                .trim_end()
                .to_string()
        })
        .ok()
}
