use anyhow::{anyhow, bail, Result};
use clap::{crate_authors, crate_description, Arg, ArgAction, ArgMatches, Command};
use loader::Loader;
use std::path::{Path, PathBuf};
use std::{env, fs, u64};
use tree_sitter_cli::highlight::ThemeConfig;
use tree_sitter_cli::input::{collect_paths, Inputs};
use tree_sitter_cli::parse::OutputFormat;
use tree_sitter_cli::{
    generate, highlight, logger, parse, playground, query, tags, test, test_highlight, test_tags,
    util, wasm,
};
use tree_sitter_config::Config;
use tree_sitter_loader as loader;

const VERSION: &str = env!("TREE_SITTER_CLI_VERSION");
const DEFAULT_GENERATE_ABI_VERSION: usize = 14;

fn main() {
    let result = run();
    if let Err(err) = &result {
        // Ignore BrokenPipe errors
        if let Some(error) = err.downcast_ref::<std::io::Error>() {
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                return;
            }
        }
        if !err.to_string().is_empty() {
            eprintln!("{:?}", err);
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let libdir_arg = Arg::new("libdir")
        .help("Path to compiled grammars folder")
        .long_help(concat!(
            "Path to compiled grammars folder.\n",
            "Can be specified with the TREE_SITTER_LIBDIR env variable"
        ))
        .long("libdir")
        .num_args(1)
        .value_name("path");

    let debug_arg = Arg::new("debug")
        .help("Show parsing debug log")
        .long("debug")
        .short('d')
        .action(ArgAction::SetTrue);

    let debug_graph_arg = Arg::new("debug-graph")
        .help("Produce the log.html file with debug graphs")
        .long("debug-graph")
        .short('D')
        .action(ArgAction::SetTrue);

    let debug_build_arg = Arg::new("debug-build")
        .help("Compile a parser in debug mode")
        .env("TREE_SITTER_DEBUG_BUILD")
        .long("debug-build")
        .short('0')
        .action(ArgAction::SetTrue);

    let paths_file_arg = Arg::new("paths-file")
        .help("The path to a file with paths to source file(s)")
        .long("paths")
        .num_args(1);

    let paths_arg = Arg::new("paths")
        .help("The source file(s) to use")
        .num_args(0..);

    let scope_arg = Arg::new("scope")
        .help("Select a language by the scope instead of a file extension")
        .env("TREE_SITTER_SCOPE")
        .long("scope")
        .value_name("source.*")
        .num_args(1);

    let limit_ranges_arg = Arg::new("limit-ranges")
        .help("Limit output to a range")
        .long("limit-range")
        .short('l')
        .num_args(1..=2)
        .value_names(["start_row:start_column", "end_row:end_column"])
        .action(ArgAction::Append);

    let time_arg = Arg::new("time")
        .help("Measure execution time")
        .long("time")
        .short('t')
        .action(ArgAction::SetTrue);

    let quiet_arg = Arg::new("quiet")
        .help("Suppress main output")
        .long("quiet")
        .short('q')
        .action(ArgAction::SetTrue);

    let app = {
        Command::new("tree-sitter")
            .author(crate_authors!("\n"))
            .about(crate_description!())
            .version(VERSION.replace("; ", "\n"))
            .subcommand_required(true)
            .disable_help_subcommand(true)
            .arg(&libdir_arg)
            .subcommand(Command::new("init-config").about("Generate a default config file"))
            .subcommand(
                Command::new("generate")
                    .alias("gen")
                    .alias("g")
                    .about("Generate a parser")
                    .arg(Arg::new("grammar-path").index(1))
                    .arg(Arg::new("log").long("log").action(ArgAction::SetTrue))
                    .arg(
                        Arg::new("abi-version")
                            .long("abi")
                            .value_name("version")
                            .help(format!(
                                concat!(
                                "Select the language ABI version to generate (default {}).\n",
                                "Use --abi=latest to generate the newest supported version ({}).",
                            ),
                                DEFAULT_GENERATE_ABI_VERSION,
                                tree_sitter::LANGUAGE_VERSION,
                            )),
                    )
                    .arg(
                        Arg::new("no-bindings")
                            .long("no-bindings")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(
                        Arg::new("build")
                            .long("build")
                            .short('b')
                            .action(ArgAction::SetTrue)
                            .help("Compile all defined languages in the current dir"),
                    )
                    .arg(&debug_build_arg)
                    .arg(
                        Arg::new("report-states-for-rule")
                            .long("report-states-for-rule")
                            .value_name("rule-name")
                            .num_args(1),
                    )
                    .arg(
                        Arg::new("no-minimize")
                            .long("no-minimize")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(libdir_arg.clone().hide(true)),
            )
            .subcommand(
                Command::new("parse")
                    .alias("p")
                    .about("Parse files")
                    .arg(&paths_file_arg)
                    .arg(&paths_arg)
                    .arg(&scope_arg)
                    .arg(
                        Arg::new("output")
                            .long("output")
                            .short('o')
                            .action(ArgAction::Set)
                            .value_parser(OutputFormat::parse)
                            .env("TREE_SITTER_PARSE_OUTPUT")
                            .default_value("s-expression")
                            .help("Output format, possible values: s-expression, cst, xml")
                            .long_help(concat!(
                                "Output format for a tree rendering\n",
                                "Possible values: s-expression, cst, xml\n",
                                "The first character of each value can be used as a shortcut"
                            ))
                    )
                    .arg(
                        Arg::new("edits")
                            .help("Plan edit on a tree")
                            .long_help(concat!(
                                "Plan edit on a tree\n",
                                "The option can be used multiple times\n",
                                "To insert newlines use a shell $'' mechanism like: --edit 0:0 0 $'Hello\\nworld'"
                            ))
                            .long("edit")
                            .short('e')
                            .num_args(3)
                            .value_names(["row:column", "delete_count", "insert_text"])
                            .action(ArgAction::Append),
                    )
                    .arg(
                        Arg::new("apply-edits")
                            .help("Apply planned edits to a tree")
                            .long_help("Apply edits that were planned with the --edit option and incrementally reparse a tree")
                            .long("apply")
                            .short('a')
                            .action(ArgAction::SetTrue),
                    )
                    .arg(&limit_ranges_arg)
                    .arg(
                        Arg::new("timeout")
                            .help("Interrupt the parsing process by timeout (µs)")
                            .long("timeout")
                            .num_args(1),
                    )
                    .arg(&debug_arg)
                    .arg(&debug_build_arg)
                    .arg(&debug_graph_arg)
                    .arg(&quiet_arg)
                    .arg(&time_arg)
                    .arg(
                        Arg::new("stat")
                            .help("Show parsing statistic")
                            .long("stat")
                            .short('s')
                            .action(ArgAction::SetTrue),
                    )
                    .arg(libdir_arg.clone().hide(true)),
            )
            .subcommand(
                Command::new("query")
                    .alias("q")
                    .about("Search files using a syntax tree query")
                    .arg(
                        Arg::new("query-path")
                            .help("Path to a file with queries")
                            .index(1)
                            .required(true),
                    )
                    .arg(&paths_arg.clone().index(2))
                    .arg(&scope_arg)
                    .arg(&paths_file_arg)
                    .arg(
                        Arg::new("captures")
                        .long("captures")
                        .short('c')
                        .action(ArgAction::SetTrue)
                        .help("Iterate over all of the individual captures in the order that they appear")
                        .long_help(concat!(
                            "Iterate over all of the individual captures in the order that they appear.\n",
                            "This is useful if you don't care about which pattern matched, and just want a single,\n",
                            "ordered sequence of captures.")
                        ))
                    .arg(
                        Arg::new("byte-range")
                            .long("byte-range")
                            .value_name("start:end")
                            .num_args(1)
                            .help("The range of byte offsets in which the query will be executed"),
                    )
                    .arg(&limit_ranges_arg)
                    .arg(Arg::new("test").long("test").action(ArgAction::SetTrue))
                    .arg(libdir_arg.clone().hide(true)),
            )
            .subcommand(
                Command::new("tags")
                    .about("Generate a list of tags")
                    .arg(&scope_arg)
                    .arg(&time_arg)
                    .arg(&quiet_arg)
                    .arg(&paths_file_arg)
                    .arg(&paths_arg)
                    .arg(libdir_arg.clone().hide(true)),
            )
            .subcommand(
                Command::new("test")
                    .alias("t")
                    .about("Run a parser's tests")
                    .arg(
                        Arg::new("filter")
                            .long("filter")
                            .short('f')
                            .num_args(1)
                            .help(
                                "Only run corpus test cases whose name includes the given string",
                            ),
                    )
                    .arg(
                        Arg::new("update").long("update").short('u').help(
                            "Update all syntax trees in corpus files with current parser output",
                        ).action(ArgAction::SetTrue),
                    )
                    .arg(&debug_arg)
                    .arg(&debug_build_arg)
                    .arg(&debug_graph_arg)
                    .arg(libdir_arg.clone().hide(true)),
            )
            .subcommand(
                Command::new("highlight")
                    .alias("hi")
                    .about("Highlight a file")
                    .arg(
                        Arg::new("html")
                            .help("Generate highlighting as an HTML document")
                            .long("html")
                            .short('H')
                            .action(ArgAction::SetTrue),
                    )
                    .arg(&scope_arg)
                    .arg(&time_arg)
                    .arg(&quiet_arg)
                    .arg(&paths_file_arg)
                    .arg(&paths_arg)
                    .arg(libdir_arg.clone().hide(true)),
            )
            .subcommand(
                Command::new("build-wasm")
                    .alias("bw")
                    .about("Compile a parser to WASM")
                    .arg(
                        Arg::new("docker")
                            .long("docker")
                            .help("Run emscripten via docker even if it is installed locally")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(Arg::new("path").index(1).num_args(0..)),
            )
            .subcommand(
                Command::new("playground")
                    .alias("play")
                    .alias("pg")
                    .alias("web-ui")
                    .about("Start local playground for a parser in the browser")
                    .arg(
                        Arg::new("quiet")
                            .long("quiet")
                            .short('q')
                            .help("Don't open in default browser")
                            .action(ArgAction::SetTrue),
                    ),
            )
            .subcommand(
                Command::new("dump-languages")
                .alias("langs")
                .about("Print info about all known language parsers"),
            )
    };

    let matches = app.get_matches();
    let libdir = matches.get_one_str("libdir");

    let current_dir = env::current_dir().unwrap();
    let config = Config::load()?;

    match matches.subcommand() {
        Some(("init-config", _)) => {
            if let Ok(Some(config_path)) = Config::find_config_file() {
                return Err(anyhow!(
                    "Remove your existing config file first: {}",
                    config_path.to_string_lossy()
                ));
            }
            let mut config = Config::initial()?;
            config.add(tree_sitter_loader::Config::initial())?;
            config.add(tree_sitter_cli::highlight::ThemeConfig::default())?;
            config.save()?;
            println!(
                "Saved initial configuration to {}",
                config.location.display()
            );
        }

        Some(("generate", matches)) => {
            let libdir = matches.get_one_str("libdir").or(libdir);
            let generate_bindings = !matches.get_flag("no-bindings");
            let debug_build = matches.get_flag("debug-build");
            let build = matches.get_flag("build");
            let grammar_path = matches.get_one_str("grammar-path");
            let report_symbol_name = matches.get_one_str("report-states-for-rule");
            if matches.get_flag("log") {
                logger::init();
            }
            let abi_version = matches.get_one_str("abi-version").map_or(
                DEFAULT_GENERATE_ABI_VERSION,
                |version| {
                    if version == "latest" {
                        tree_sitter::LANGUAGE_VERSION
                    } else {
                        version.parse().expect("invalid abi version flag")
                    }
                },
            );
            generate::generate_parser_in_directory(
                &current_dir,
                grammar_path,
                abi_version,
                generate_bindings,
                report_symbol_name,
            )?;
            if build {
                let mut loader = loader_with_libdir(libdir)?;
                loader.use_debug_build(debug_build);
                loader.languages_at_path(&current_dir)?;
            }
        }

        Some(("test", matches)) => {
            let libdir = matches.get_one_str("libdir").or(libdir);
            let debug = matches.get_flag("debug");
            let debug_graph = matches.get_flag("debug-graph");
            let debug_build = matches.get_flag("debug-build");
            let update = matches.get_flag("update");
            let filter = matches.get_one_str("filter");

            if debug {
                // For augmenting debug logging in external scanners
                env::set_var("TREE_SITTER_DEBUG", "1");
            }

            let mut loader = loader_with_libdir(libdir)?;
            loader.use_debug_build(debug_build);

            let languages = loader.languages_at_path(&current_dir)?;
            let language = languages
                .first()
                .ok_or_else(|| anyhow!("No language found"))?;
            let test_dir = current_dir.join("test");

            // Run the corpus tests. Look for them at two paths: `test/corpus` and `corpus`.
            let mut test_corpus_dir = test_dir.join("corpus");
            if !test_corpus_dir.is_dir() {
                test_corpus_dir = current_dir.join("corpus");
            }
            if test_corpus_dir.is_dir() {
                test::run_tests_at_path(
                    *language,
                    &test_corpus_dir,
                    debug,
                    debug_graph,
                    filter,
                    update,
                )?;
            }

            // Check that all of the queries are valid.
            test::check_queries_at_path(*language, &current_dir.join("queries"))?;

            // Run the syntax highlighting tests.
            let test_highlight_dir = test_dir.join("highlight");
            if test_highlight_dir.is_dir() {
                test_highlight::test_highlights(&loader, &test_highlight_dir)?;
            }

            let test_tag_dir = test_dir.join("tags");
            if test_tag_dir.is_dir() {
                test_tags::test_tags(&loader, &test_tag_dir)?;
            }
        }

        Some(("parse", matches)) => {
            let libdir = matches.get_one_str("libdir").or(libdir);
            let output = matches.get_one::<OutputFormat>("output");
            let scope = matches.get_one_str("scope");
            let edits = matches.get_many_str("edits");
            let edits = edits.as_ref().map(Vec::as_ref);
            let apply_edits = matches.get_flag("apply-edits");
            let limit_ranges = matches.get_occurrences_str("limit-ranges");
            let limit_ranges = limit_ranges.as_ref().map(|v| v.as_ref().map(Vec::as_ref));
            let debug = matches.get_flag("debug");
            let debug_build = matches.get_flag("debug-build");
            let debug_graph = matches.get_flag("debug-graph");
            let quiet = matches.get_flag("quiet");
            let time = matches.get_flag("time");
            let mut stats = matches.get_flag("stat").then(|| parse::Stats::default());
            let inputs = Inputs::collect(
                matches.get_one_str("paths-file"),
                matches.get_many_str("paths").map(IntoIterator::into_iter),
            )?;

            if inputs.len() > 1 {
                if limit_ranges.is_some() {
                    bail!("The `--limit-range, -l` option currently only supported with a one input item");
                }
                if edits.is_some() {
                    bail!("The `--edit, -e` option currently only supported with a one input item");
                }
            }

            let max_path_length = inputs.max_path_length();
            let mut show_file_names = inputs.len();
            if show_file_names == 1 {
                show_file_names = 0;
            }

            let timeout = matches
                .get_one::<String>("timeout")
                .map_or(0, |t| u64::from_str_radix(t, 10).unwrap());
            let cancellation_flag = util::cancel_on_stdin();

            if debug {
                // For augmenting debug logging in external scanners
                env::set_var("TREE_SITTER_DEBUG", "1");
            }

            let mut loader = loader_with_libdir(libdir)?;
            loader.use_debug_build(debug_build);

            let mut has_error = false;
            let loader_config = config.get()?;
            loader.find_all_languages(&loader_config)?;

            for input in inputs.into_parser_inputs(&mut loader, scope, Some(&current_dir)) {
                let this_file_errored = parse::parse_input(
                    input?,
                    output,
                    edits,
                    apply_edits,
                    limit_ranges,
                    time,
                    quiet,
                    debug,
                    debug_graph,
                    timeout,
                    Some(&cancellation_flag),
                    max_path_length,
                    show_file_names,
                )?;
                if show_file_names > 0 {
                    show_file_names -= 1;
                }

                if let Some(stats) = &mut stats {
                    stats.total_parses += 1;
                    if !this_file_errored {
                        stats.successful_parses += 1;
                    }
                }

                has_error |= this_file_errored;
            }

            if let Some(stats) = stats {
                println!("{}", stats)
            }

            if has_error {
                return Err(anyhow!(""));
            }
        }

        Some(("query", matches)) => {
            let libdir = matches.get_one_str("libdir").or(libdir);
            let scope = matches.get_one_str("scope");
            let captures = matches.get_flag("captures");
            let should_test = matches.get_flag("test");
            let query_path = Path::new(matches.get_one_str("query-path").unwrap());
            let paths = collect_paths(
                matches.get_one::<String>("paths-file").map(|s| &**s),
                matches.get_many_str("paths").map(IntoIterator::into_iter),
            )?;
            let range = matches.get_one_str("byte-range").map(|br| {
                let r: Vec<&str> = br.split(":").collect();
                r[0].parse().unwrap()..r[1].parse().unwrap()
            });
            let limit_ranges = matches.get_occurrences_str("limit-ranges");
            let limit_ranges = limit_ranges.as_ref().map(|v| v.as_ref().map(Vec::as_ref));

            let loader_config = config.get()?;
            let mut loader = loader_with_libdir(libdir)?;
            loader.find_all_languages(&loader_config)?;
            let language =
                loader.select_language(Some(&current_dir), scope, Some(Path::new(&paths[0])))?;
            query::query_files_at_paths(
                language,
                paths,
                query_path,
                captures,
                range,
                limit_ranges,
                should_test,
            )?;
        }

        Some(("tags", matches)) => {
            let libdir = matches.get_one_str("libdir").or(libdir);
            let scope = matches.get_one_str("scope");
            let quiet = matches.get_flag("quiet");
            let time = matches.get_flag("time");
            let loader_config = config.get()?;
            let mut loader = loader_with_libdir(libdir)?;
            loader.find_all_languages(&loader_config)?;
            let paths = collect_paths(
                matches.get_one_str("paths-file"),
                matches.get_many_str("paths").map(IntoIterator::into_iter),
            )?;
            tags::generate_tags(&loader, scope, &paths, quiet, time)?;
        }

        Some(("highlight", matches)) => {
            let libdir = matches.get_one_str("libdir").or(libdir);
            let time = matches.get_flag("time");
            let quiet = matches.get_flag("quiet");
            let html_mode = quiet || matches.get_flag("html");
            let paths = collect_paths(
                matches.get_one_str("paths-file"),
                matches.get_many_str("paths").map(IntoIterator::into_iter),
            )?;

            let loader_config = config.get()?;
            let theme_config: ThemeConfig = config.get()?;
            let mut loader = loader_with_libdir(libdir)?;
            loader.configure_highlights(&theme_config.theme.highlight_names);
            loader.find_all_languages(&loader_config)?;

            let cancellation_flag = util::cancel_on_stdin();

            let mut lang = None;
            if let Some(scope) = matches.get_one_str("scope") {
                lang = loader.language_configuration_for_scope(scope)?;
                if lang.is_none() {
                    return Err(anyhow!("Unknown scope '{}'", scope));
                }
            }

            if html_mode && !quiet {
                println!("{}", highlight::HTML_HEADER);
            }

            for path in paths {
                let path = Path::new(&path);
                let (language, language_config) = match lang {
                    Some(v) => v,
                    None => match loader.language_configuration_for_file_name(path)? {
                        Some(v) => v,
                        None => {
                            eprintln!("No language found for path {:?}", path);
                            continue;
                        }
                    },
                };

                if let Some(highlight_config) = language_config.highlight_config(language)? {
                    let source = fs::read(path)?;
                    if html_mode {
                        highlight::html(
                            &loader,
                            &theme_config.theme,
                            &source,
                            highlight_config,
                            quiet,
                            time,
                            Some(&cancellation_flag),
                        )?;
                    } else {
                        highlight::ansi(
                            &loader,
                            &theme_config.theme,
                            &source,
                            highlight_config,
                            time,
                            Some(&cancellation_flag),
                        )?;
                    }
                } else {
                    eprintln!("No syntax highlighting config found for path {:?}", path);
                }
            }

            if html_mode && !quiet {
                println!("{}", highlight::HTML_FOOTER);
            }
        }

        Some(("build-wasm", matches)) => {
            let grammar_path = current_dir.join(matches.get_one_str("path").unwrap_or(""));
            wasm::compile_language_to_wasm(&grammar_path, matches.get_flag("docker"))?;
        }

        Some(("playground", matches)) => {
            let open_in_browser = !matches.get_flag("quiet");
            playground::serve(&current_dir, open_in_browser);
        }

        Some(("dump-languages", _)) => {
            let loader_config = config.get()?;
            let mut loader = loader_with_libdir(None)?;
            loader.find_all_languages(&loader_config)?;
            for (configuration, language_path) in loader.get_all_language_configurations() {
                println!(
                    concat!(
                        "scope: {}\n",
                        "parser: {:?}\n",
                        "highlights: {:?}\n",
                        "file_types: {:?}\n",
                        "content_regex: {:?}\n",
                        "injection_regex: {:?}\n",
                    ),
                    configuration.scope.as_ref().unwrap_or(&String::new()),
                    language_path,
                    configuration.highlights_filenames,
                    configuration.file_types,
                    configuration.content_regex,
                    configuration.injection_regex,
                );
            }
        }

        Some((a, b)) => println!("{a:?} -- {b:?}"),
        None => println!("None."),
    }

    Ok(())
}

fn loader_with_libdir(libdir: Option<&str>) -> Result<Loader> {
    if let Some(libdir) = libdir {
        Ok(Loader::with_parser_lib_path(PathBuf::from(libdir)))
    } else {
        Loader::new()
    }
}

trait ArgStr<'s> {
    fn get_one_str(&'s self, id: &str) -> Option<&'s str>;
    fn get_many_str(&'s self, id: &str) -> Option<Vec<&'s str>>;
    fn get_occurrences_str(&'s self, id: &str) -> Option<Vec<Vec<&'s str>>>;
}

impl<'s> ArgStr<'s> for ArgMatches {
    fn get_one_str(&'s self, id: &str) -> Option<&'s str> {
        self.get_one::<String>(id).map(|s| &**s)
    }

    fn get_many_str(&'s self, id: &str) -> Option<Vec<&'s str>> {
        self.get_many::<String>(id)
            .map(|v| v.map(|s| &**s).collect())
    }

    fn get_occurrences_str(&'s self, id: &str) -> Option<Vec<Vec<&'s str>>> {
        self.get_occurrences::<String>(id)
            .map(|v| v.map(|o| o.into_iter().map(|s| &**s).collect()).collect())
    }
}
