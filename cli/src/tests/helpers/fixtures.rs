use anyhow::Result;
use lazy_static::lazy_static;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{env, fs};
use tree_sitter::Language;
use tree_sitter_highlight::HighlightConfiguration;
use tree_sitter_loader::Loader;
use tree_sitter_tags::TagsConfiguration;

include!("./dirs.rs");

lazy_static! {
    static ref TEST_LOADER: Loader = {
        let mut loader = Loader::with_parser_lib_path(SCRATCH_DIR.clone());
        if env::var("TREE_SITTER_GRAMMAR_DEBUG").is_ok() {
            loader.use_debug_build(true);
        }
        loader
    };
}

pub fn test_loader<'a>() -> &'a Loader {
    &*TEST_LOADER
}

pub fn fixtures_dir<'a>() -> &'static Path {
    &FIXTURES_DIR
}

pub fn get_language(name: &str) -> Language {
    TEST_LOADER
        .load_language_at_path(&GRAMMARS_DIR.join(name).join("src"), &HEADER_DIR, &None)
        .unwrap()
}

pub fn get_language_queries_path(language_name: &str) -> PathBuf {
    GRAMMARS_DIR.join(language_name).join("queries")
}

pub fn get_highlight_config(
    language_name: &str,
    injection_query_filename: Option<&str>,
    highlight_names: &[String],
) -> HighlightConfiguration {
    let language = get_language(language_name);
    let queries_path = get_language_queries_path(language_name);
    let highlights_query = fs::read_to_string(queries_path.join("highlights.scm")).unwrap();
    let injections_query = if let Some(injection_query_filename) = injection_query_filename {
        fs::read_to_string(queries_path.join(injection_query_filename)).unwrap()
    } else {
        String::new()
    };
    let locals_query = fs::read_to_string(queries_path.join("locals.scm")).unwrap_or(String::new());
    let mut result = HighlightConfiguration::new(
        language,
        language_name,
        &highlights_query,
        &injections_query,
        &locals_query,
        false,
    )
    .unwrap();
    result.configure(&highlight_names);
    result
}

pub fn get_tags_config(language_name: &str) -> TagsConfiguration {
    let language = get_language(language_name);
    let queries_path = get_language_queries_path(language_name);
    let tags_query = fs::read_to_string(queries_path.join("tags.scm")).unwrap();
    let locals_query = fs::read_to_string(queries_path.join("locals.scm")).unwrap_or(String::new());
    TagsConfiguration::new(language, &tags_query, &locals_query).unwrap()
}

pub fn get_test_language(name: &str, parser_code: &str, path: Option<&Path>) -> Language {
    let parser_c_path = SCRATCH_DIR.join(&format!("{}-parser.c", name));
    if !fs::read_to_string(&parser_c_path)
        .map(|content| content == parser_code)
        .unwrap_or(false)
    {
        fs::write(&parser_c_path, parser_code).unwrap();
    }
    let scanner_path = path.and_then(|p| {
        let result = p.join("scanner.c");
        if result.exists() {
            Some(result)
        } else {
            None
        }
    });

    // let needs_recompile =
    //     needs_recompile(&FIXTURES_DIR.join(name), &parser_c_path, &scanner_path, &[]).unwrap();

    TEST_LOADER
        .load_language_from_sources(
            name,
            &HEADER_DIR,
            &parser_c_path,
            scanner_path.as_deref(),
            false,
        )
        .unwrap()
}

pub fn get_test_grammar(name: &str) -> (String, Option<PathBuf>) {
    let dir = fixtures_dir().join("test_grammars").join(name);
    let grammar = fs::read_to_string(&dir.join("grammar.json")).expect(&format!(
        "Can't find grammar.json for test grammar {}",
        name
    ));
    (grammar, Some(dir))
}

pub fn needs_recompile(
    lib_path: &Path,
    parser_c_path: &Path,
    scanner_path: &Option<PathBuf>,
    external_files_paths: &[PathBuf],
) -> Result<bool> {
    if !lib_path.exists() {
        return Ok(true);
    }
    let lib_mtime = mtime(lib_path)?;
    if mtime(parser_c_path)? > lib_mtime {
        return Ok(true);
    }
    if let Some(scanner_path) = scanner_path {
        if mtime(scanner_path)? > lib_mtime {
            return Ok(true);
        }
    }
    for path in external_files_paths {
        if mtime(path)? > lib_mtime {
            return Ok(true);
        }
    }
    Ok(false)
}

fn mtime(path: &Path) -> Result<SystemTime> {
    Ok(fs::metadata(path)?.modified()?)
}
