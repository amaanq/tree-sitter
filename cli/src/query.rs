use crate::{
    query_testing,
    render::{self, Colors, NodeRangeCheck, ScopeRange},
};
use ansi_term::{Color, Style};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashSet,
    fs,
    io::{self, Write},
    ops::Range,
    path::{Path, PathBuf},
    thread,
    time::Instant,
};
use tree_sitter::{Language, Parser, Point, Query, QueryCapture, QueryCursor};

pub fn query_files_at_paths(
    language: Language,
    paths: Vec<PathBuf>,
    query_path: &Path,
    ordered_captures: bool,
    range: Option<Range<usize>>,
    limit_ranges: &Option<Vec<Vec<&str>>>,
    should_test: bool,
    quiet: bool,
    print_time: bool,
) -> Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let query_source = fs::read_to_string(query_path)
        .with_context(|| format!("Error reading query file {:?}", query_path))?;
    let query = Query::new(language, &query_source).with_context(|| "Query compilation failed")?;

    let max_capture_name_len =
        query
            .capture_names()
            .iter()
            .fold(0usize, |acc, e| if e.len() > acc { e.len() } else { acc });

    let mut query_cursor = QueryCursor::new();
    if let Some(range) = range {
        query_cursor.set_byte_range(range);
    }

    let mut parser = Parser::new();
    parser.set_language(language)?;

    let c = render::Colors::new();
    let name_color = Color::RGB(38, 166, 154);

    let mut limit_ranges = {
        let limit_ranges = limit_ranges
            .as_ref()
            .map(|limit_ranges| ScopeRange::parse_inputs(&limit_ranges))
            .transpose()?;

        if limit_ranges.is_some() && paths.len() > 1 {
            bail!("The `--limit-range` currently only supported with a one input item");
        }

        limit_ranges
    };

    let mut show_file_names = paths.len();
    if show_file_names == 1 {
        show_file_names = 0;
    }

    for path in paths {
        let mut results = Vec::new();

        let source_code =
            fs::read(&path).with_context(|| format!("Error reading source file {:?}", path))?;
        let source_code = source_code.as_slice();

        let scope = thread::scope(|s| {
            let counts = s.spawn(|| bytecount::count(source_code, b'\n'));
            (
                parser.parse(&source_code, None).unwrap(),
                counts.join().expect("Can't start a thread"),
            )
        });
        let (tree, lines_count) = scope;
        let pos_align = format!("{lines_count}:xxx - {lines_count}:xxx").len();

        if show_file_names > 0 {
            writeln!(
                &mut stdout,
                "{C}{}{R}",
                path.to_string_lossy(),
                C = name_color.prefix(),
                R = name_color.suffix()
            )?;
        }

        let mut tree_cursor = tree.walk();

        let mut last_row = usize::MAX;

        let start = Instant::now();
        if ordered_captures {
            for (m, capture_index) in query_cursor.captures(&query, tree.root_node(), source_code) {
                let pattern_index = m.pattern_index;
                let capture = m.captures[capture_index];

                let check = NodeRangeCheck::check_parent_scoped(
                    &mut tree_cursor,
                    &mut limit_ranges,
                    &capture.node,
                )?;
                if check.draw_extra_lf {
                    println!();
                }
                if check.hide_row {
                    continue;
                }

                let capture_index = capture.index;
                let capture_name = &query.capture_names()[capture_index as usize];
                let (pos, pos_c, ml) = format_pos(&capture, &mut last_row, &c);
                let capture_text = capture.node.utf8_text(&source_code).unwrap_or("");
                if !quiet {
                    let text = if ml {
                        let capture_text = capture_text.lines().next().unwrap();
                        format!(
                            "{BK}`{CT}{capture_text}{BK}`{R}...",
                            CT = c.text.prefix(),
                            BK = c.backtick.prefix(),
                            R = c.backtick.suffix()
                        )
                    } else {
                        format!(
                            "{BK}`{CT}{capture_text}{BK}`{R}",
                            CT = c.text.prefix(),
                            BK = c.backtick.prefix(),
                            R = c.backtick.suffix()
                        )
                    };
                    #[rustfmt::skip]
                    writeln!(
                        &mut stdout,
                        "{P}{pos:<pos_align$} {PI}{pi:>2}{CL}:{CI}{ci:<3} {CN}{cn:<max_cn$} {text}",
                        pi=pattern_index, ci=capture_index, cn=capture_name, max_cn=max_capture_name_len,
                        P=pos_c.prefix(), PI=c.field.prefix(), CL=c.text.prefix(), CI=c.nonterm.prefix(), CN=c.bytes.prefix(),
                    )?;
                }
                results.push(query_testing::CaptureInfo {
                    name: capture_name.to_string(),
                    start: capture.node.start_position(),
                    end: capture.node.end_position(),
                });
            }
        } else {
            let mut hidden_matches = HashSet::new();

            for m in query_cursor.matches(&query, tree.root_node(), source_code) {
                let mut capture_pad = "";
                let max_capture_name_len2 = max_capture_name_len + 1;
                let mut pattern_index = usize::MAX;
                if m.captures.len() == 0 {
                    if !hidden_matches.contains(&m.id()) {
                        hidden_matches.insert(m.id());
                        writeln!(&mut stdout, "Hidden match with id: {}", m.id())?;
                        writeln!(
                            &mut stdout,
                            "You need to specify al least one capture to have an output for it"
                        )?;
                    }
                }
                for capture in m.captures {
                    let check = NodeRangeCheck::check_parent_scoped(
                        &mut tree_cursor,
                        &mut limit_ranges,
                        &capture.node,
                    )?;
                    if check.draw_extra_lf {
                        println!();
                    }
                    if check.hide_row {
                        continue;
                    }

                    let pat_c = if pattern_index == usize::MAX {
                        pattern_index = m.pattern_index;
                        c.field
                    } else {
                        capture_pad = " ";
                        c.pos2
                    };
                    let capture_index = capture.index;
                    let capture_name = &query.capture_names()[capture_index as usize];
                    let (pos, pos_c, ml) = format_pos(capture, &mut last_row, &c);
                    let capture_text = capture.node.utf8_text(&source_code).unwrap_or("");
                    if !quiet {
                        let text = if ml {
                            let capture_text = capture_text.lines().next().unwrap();
                            format!(
                                "{BK}`{CT}{capture_text}{BK}`{R}...",
                                CT = c.text.prefix(),
                                BK = c.backtick.prefix(),
                                R = c.backtick.suffix()
                            )
                        } else {
                            format!(
                                "{BK}`{CT}{capture_text}{BK}`{R}",
                                CT = c.text.prefix(),
                                BK = c.backtick.prefix(),
                                R = c.backtick.suffix()
                            )
                        };
                        let capture_name = format!("{capture_pad}{capture_name}");
                        #[rustfmt::skip]
                        writeln!(
                                &mut stdout,
                                "{P}{pos:<pos_align$} {PI}{pi:>3}{CL}:{CI}{ci:<3} {CN}{cn:<max_cn$} {text}",
                                pi=pattern_index, ci=capture_index, cn=capture_name, max_cn=max_capture_name_len2,
                                P=pos_c.prefix(), PI=pat_c.prefix(), CL=c.text.prefix(), CI=c.nonterm.prefix(), CN=c.bytes.prefix(),
                            )?;
                    }
                    results.push(query_testing::CaptureInfo {
                        name: capture_name.to_string(),
                        start: capture.node.start_position(),
                        end: capture.node.end_position(),
                    });
                }
            }
        }
        if query_cursor.did_exceed_match_limit() {
            writeln!(
                &mut stdout,
                "  WARNING: Query exceeded maximum number of in-progress captures!"
            )?;
        }
        if should_test {
            query_testing::assert_expected_captures(results, path, &mut parser, language)?
        }
        if show_file_names > 1 {
            println!()
        }
        if show_file_names > 0 {
            show_file_names -= 1;
        }
        if print_time {
            writeln!(&mut stdout, "{:?}", start.elapsed())?;
        }
    }

    Ok(())
}

fn format_pos(
    capture: &QueryCapture,
    last_row: &mut usize,
    colors: &Colors,
) -> (String, Style, bool) {
    let Point {
        row: start_row,
        column: start_column,
    } = capture.node.start_position();
    let Point {
        row: end_row,
        column: end_column,
    } = capture.node.end_position();
    let pos_c = if start_row != *last_row {
        *last_row = start_row;
        colors.pos1
    } else {
        colors.pos2
    };
    (
        format!("{start_row}:{start_column:<3} - {end_row}:{end_column}"),
        pos_c,
        end_row > start_row,
    )
}

// TODO: query rendering improvements
//   [ ] Implement a multiline capture_text unfolding with correct pos per line and backticked every line line in parsing tree
//   [ ] Add a flag to control how to show capture_text like unfolded or not.
//   [ ] Show an input name only if there multiple inputs.
