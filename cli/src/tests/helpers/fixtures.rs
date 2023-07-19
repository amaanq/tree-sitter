use lazy_static::lazy_static;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::Language;
use tree_sitter_highlight::HighlightConfiguration;
use tree_sitter_loader::Loader;
use tree_sitter_tags::TagsConfiguration;

include!("./dirs.rs");

lazy_static! {
    static ref TEST_LOADER: Loader = Loader::with_parser_lib_path(SCRATCH_DIR.clone());
}

pub fn test_loader<'a>() -> &'a Loader {
    &*TEST_LOADER
}

pub fn fixtures_dir<'a>() -> &'static Path {
    &FIXTURES_DIR
}

fn get_embedded_language(name: &str) -> Option<Language> {
    macro_rules! L {
        ($language: expr) => {
            unsafe { std::mem::transmute($language) }
        };
    }

    #[rustfmt::skip]
    let language = match name {
        "bash"              => L!(tree_sitter_bash::language()),
        "c"                 => L!(tree_sitter_c::language()),
        "cpp"               => L!(tree_sitter_cpp::language()),
        "embedded-template" => L!(tree_sitter_embedded_template::language()),
        "go"                => L!(tree_sitter_go::language()),
        "html"              => L!(tree_sitter_html::language()),
        "java"              => L!(tree_sitter_java::language()),
        "javascript"        => L!(tree_sitter_javascript::language()),
        "jsdoc"             => L!(tree_sitter_jsdoc::language()),
        "json"              => L!(tree_sitter_json::language()),
        "php"               => L!(tree_sitter_php::language()),
        "python"            => L!(tree_sitter_python::language()),
        "ruby"              => L!(tree_sitter_ruby::language()),
        "rust"              => L!(tree_sitter_rust::language()),
        "typescript"        => L!(tree_sitter_typescript::language_typescript()),
        _ => return None,
    };
    Some(language)
}

pub fn get_language(name: &str) -> Language {
    #[cfg(not(target_env = "musl"))]
    {
        TEST_LOADER
            .load_language_at_path(&GRAMMARS_DIR.join(name).join("src"), &HEADER_DIR)
            .unwrap()
    }

    #[cfg(target_env = "musl")]
    {
        if let Some(language) = get_embedded_language(name) {
            language
        } else {
            panic!("Unsupported language: {}", name)
        }
    }
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
        &highlights_query,
        &injections_query,
        &locals_query,
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
    TEST_LOADER
        .load_language_from_sources(name, &HEADER_DIR, &parser_c_path, &scanner_path)
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
