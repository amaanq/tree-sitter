use std::{collections::HashMap, env, fs, path::Path};

use lazy_static::lazy_static;
use rand::Rng;
use tree_sitter::{Language, Parser};

pub mod allocations;
pub mod corpus_test;
pub mod edits;
pub mod random;
pub mod scope_sequence;

use crate::{
    fuzz::{
        corpus_test::{
            check_changed_ranges, check_consistent_sizes, get_parser, set_included_ranges,
        },
        edits::{get_random_edit, invert_edit},
        random::Rand,
    },
    parse::perform_edit,
    test::{parse_tests, print_diff, print_diff_key, strip_sexp_fields, TestEntry},
};

lazy_static! {
    pub static ref LOG_ENABLED: bool = env::var("TREE_SITTER_LOG").is_ok();
    pub static ref LOG_GRAPH_ENABLED: bool = env::var("TREE_SITTER_LOG_GRAPHS").is_ok();
    pub static ref LANGUAGE_FILTER: Option<String> = env::var("TREE_SITTER_LANGUAGE").ok();
    pub static ref EXAMPLE_FILTER: Option<String> = env::var("TREE_SITTER_EXAMPLE").ok();
    pub static ref START_SEED: usize = new_seed();
    pub static ref EDIT_COUNT: usize = int_env_var("TREE_SITTER_EDITS").unwrap_or(3);
    pub static ref ITERATION_COUNT: usize = int_env_var("TREE_SITTER_ITERATIONS").unwrap_or(10);
}

fn int_env_var(name: &'static str) -> Option<usize> {
    env::var(name).ok().and_then(|e| e.parse().ok())
}

pub fn new_seed() -> usize {
    int_env_var("TREE_SITTER_SEED").unwrap_or_else(|| {
        let mut rng = rand::thread_rng();
        rng.gen::<usize>()
    })
}

pub fn fuzz_language_corpus(
    language: &Language,
    language_name: &str,
    start_seed: usize,
    skipped: Option<&[String]>,
    grammar_dir: &Path,
    subdir: Option<&str>,
) {
    let subdir = subdir.unwrap_or_default();

    let corpus_dir = grammar_dir.join(subdir).join("test").join("corpus");

    let main_tests = parse_tests(&corpus_dir).unwrap();
    let tests = flatten_tests(main_tests);

    let mut skipped = skipped.map(|x| {
        x.iter()
            .map(|x| (x.as_str(), 0))
            .collect::<HashMap<&str, usize>>()
    });

    let mut failure_count = 0;

    let log_seed = env::var("TREE_SITTER_LOG_SEED").is_ok();
    let dump_edits = env::var("TREE_SITTER_DUMP_EDITS").is_ok();

    if log_seed {
        println!("  start seed: {start_seed}");
    }

    println!();
    for (test_index, test) in tests.iter().enumerate() {
        let test_name = format!("{language_name} - {}", test.name);
        if let Some(skipped) = skipped.as_mut() {
            if let Some(counter) = skipped.get_mut(test_name.as_str()) {
                println!("  {test_index}. {test_name} - SKIPPED");
                *counter += 1;
                continue;
            }
        }

        println!("  {test_index}. {test_name}");

        let passed = allocations::record(|| {
            let mut log_session = None;
            let mut parser = get_parser(&mut log_session, "log.html");
            parser.set_language(language).unwrap();
            set_included_ranges(&mut parser, &test.input, test.template_delimiters);

            let tree = parser.parse(&test.input, None).unwrap();
            let mut actual_output = tree.root_node().to_sexp();
            if !test.has_fields {
                actual_output = strip_sexp_fields(&actual_output);
            }

            if actual_output != test.output {
                println!("Incorrect initial parse for {test_name}");
                print_diff_key();
                print_diff(&actual_output, &test.output);
                println!();
                return false;
            }

            true
        });

        if !passed {
            failure_count += 1;
            continue;
        }

        let mut parser = Parser::new();
        parser.set_language(language).unwrap();
        let tree = parser.parse(&test.input, None).unwrap();
        println!("TREE1\n{:#}", tree.root_node());
        drop(parser);

        for trial in 0..*ITERATION_COUNT {
            let seed = start_seed + trial;
            let passed = allocations::record(|| {
                let mut rand = Rand::new(seed);
                let mut log_session = None;
                let mut parser = get_parser(&mut log_session, "log.html");
                parser.set_language(language).unwrap();
                let mut tree = tree.clone();
                let mut input = test.input.clone();

                if *LOG_GRAPH_ENABLED {
                    eprintln!("{}\n", String::from_utf8_lossy(&input));
                }

                // Perform a random series of edits and reparse.
                let mut undo_stack = Vec::new();
                for _ in 0..=rand.unsigned(*EDIT_COUNT) {
                    let edit = get_random_edit(&mut rand, &input);
                    undo_stack.push(invert_edit(&input, &edit));
                    perform_edit(&mut tree, &mut input, &edit).unwrap();
                    println!("{edit:?}");
                }

                if log_seed {
                    println!("   {test_index}.{trial:<2} seed: {seed}");
                }

                if dump_edits {
                    fs::create_dir_all("fuzz").unwrap();
                    fs::write(
                        Path::new("fuzz").join(
                            format!("edit.{seed}.{test_index}.{trial} {test_name}")
                                .replace('/', "_"),
                        ),
                        &input,
                    )
                    .unwrap();
                }

                if *LOG_GRAPH_ENABLED {
                    eprintln!("{}\n", String::from_utf8_lossy(&input));
                }

                set_included_ranges(&mut parser, &input, test.template_delimiters);
                let mut tree2 = parser.parse(&input, Some(&tree)).unwrap();
                println!("TREE2\n{:#}", tree2.root_node());

                // Check that the new tree is consistent.
                check_consistent_sizes(&tree2, &input);
                if let Err(message) = check_changed_ranges(&tree, &tree2, &input) {
                    println!("\n[1]Unexpected scope change in seed {seed} with start seed {start_seed}\n{message}\n\n");
                    return false;
                }

                // Undo all of the edits and re-parse again.
                while let Some(edit) = undo_stack.pop() {
                    perform_edit(&mut tree2, &mut input, &edit).unwrap();
                    println!("{edit:?}");
                }
                if *LOG_GRAPH_ENABLED {
                    eprintln!("{}\n", String::from_utf8_lossy(&input));
                }

                set_included_ranges(&mut parser, &test.input, test.template_delimiters);
                let tree3 = parser.parse(&input, Some(&tree2)).unwrap();

                // Verify that the final tree matches the expectation from the corpus.
                let mut actual_output = tree3.root_node().to_sexp();
                if !test.has_fields {
                    actual_output = strip_sexp_fields(&actual_output);
                }

                if actual_output != test.output {
                    println!("Incorrect parse for {test_name} - seed {seed}");
                    print_diff_key();
                    print_diff(&actual_output, &test.output);
                    println!();
                    return false;
                }

                // Check that the edited tree is consistent.
                check_consistent_sizes(&tree3, &input);
                if let Err(message) = check_changed_ranges(&tree2, &tree3, &input) {
                    println!("[2]Unexpected scope change in seed {seed} with start seed {start_seed}\n{message}\n\n");
                    return false;
                }

                true
            });

            if !passed {
                failure_count += 1;
                break;
            }
        }
    }

    assert!(
        failure_count == 0,
        "{failure_count} {language_name} corpus tests failed"
    );

    if let Some(skipped) = skipped.as_mut() {
        skipped.retain(|_, v| *v == 0);

        if !skipped.is_empty() {
            println!("Non matchable skip definitions:");
            for k in skipped.keys() {
                println!("  {k}");
            }
            panic!("Non matchable skip definitions needs to be removed");
        }
    }
}

pub struct FlattenedTest {
    pub name: String,
    pub input: Vec<u8>,
    pub output: String,
    pub has_fields: bool,
    pub template_delimiters: Option<(&'static str, &'static str)>,
}

pub fn flatten_tests(test: TestEntry) -> Vec<FlattenedTest> {
    fn helper(test: TestEntry, is_root: bool, prefix: &str, result: &mut Vec<FlattenedTest>) {
        match test {
            TestEntry::Example {
                mut name,
                input,
                output,
                has_fields,
                ..
            } => {
                if !prefix.is_empty() {
                    name.insert_str(0, " - ");
                    name.insert_str(0, prefix);
                }
                if let Some(filter) = EXAMPLE_FILTER.as_ref() {
                    if !name.contains(filter.as_str()) {
                        return;
                    }
                }
                result.push(FlattenedTest {
                    name,
                    input,
                    output,
                    has_fields,
                    template_delimiters: None,
                });
            }
            TestEntry::Group {
                mut name, children, ..
            } => {
                if !is_root && !prefix.is_empty() {
                    name.insert_str(0, " - ");
                    name.insert_str(0, prefix);
                }
                for child in children {
                    helper(child, false, &name, result);
                }
            }
        }
    }
    let mut result = Vec::new();
    helper(test, true, "", &mut result);
    result
}
