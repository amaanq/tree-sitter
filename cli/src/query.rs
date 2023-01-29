use crate::{query_testing, render};
use anyhow::{Context, Result};
use std::{
    fs,
    io::{self, Write},
    ops::Range,
    path::{Path, PathBuf},
};
use tree_sitter::{Language, Parser, Point, Query, QueryCursor};

pub fn query_files_at_paths(
    language: Language,
    paths: Vec<PathBuf>,
    query_path: &Path,
    ordered_captures: bool,
    range: Option<Range<usize>>,
    should_test: bool,
) -> Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let query_source = fs::read_to_string(query_path)
        .with_context(|| format!("Error reading query file {:?}", query_path))?;
    let query = Query::new(language, &query_source).with_context(|| "Query compilation failed")?;

    let max_capture_len =
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

    for path in paths {
        let mut results = Vec::new();

        writeln!(&mut stdout, "{}", path.to_string_lossy())?;

        let source_code =
            fs::read(&path).with_context(|| format!("Error reading source file {:?}", path))?;
        let tree = parser.parse(&source_code, None).unwrap();

        let mut last_row = usize::MAX;

        if ordered_captures {
            for (m, capture_index) in
                query_cursor.captures(&query, tree.root_node(), source_code.as_slice())
            {
                let pattern_index = m.pattern_index;
                let capture = m.captures[capture_index];
                let capture_index = capture.index;
                let capture_name = &query.capture_names()[capture_index as usize];
                let Point {
                    row: start_row,
                    column: start_column,
                } = capture.node.start_position();
                let Point {
                    row: end_row,
                    column: end_column,
                } = capture.node.end_position();
                let pos_c = if start_row != last_row {
                    last_row = start_row;
                    c.pos1
                } else {
                    c.pos2
                };
                let pos = format!("{start_row}:{start_column:<3} - {end_row}:{end_column}");
                let capture_text = capture.node.utf8_text(&source_code).unwrap_or("");
                writeln!(
                    &mut stdout,
                    // "{pos:<15} {pattern_index:>2}:{capture_index:<2} {capture_name:>20} `{capture_text}`"
                    "{P}{pos:<18} {PI}{pattern_index:>3}{CL}:{CI}{capture_index:<3} {CN}{capture_name:<max_capture_len$} {BK}`{CT}{capture_text}{BK}`{R}",
                    P=pos_c.prefix(), PI=c.field.prefix(), CL=c.text.prefix(), CI=c.nonterm.prefix(),
                    CN=c.bytes.prefix(), CT=c.text.prefix(), BK=c.backtick.prefix(), R=c.backtick.suffix(),
                )?;
                results.push(query_testing::CaptureInfo {
                    name: capture_name.to_string(),
                    start: capture.node.start_position(),
                    end: capture.node.end_position(),
                });
            }
        } else {
            for m in query_cursor.matches(&query, tree.root_node(), source_code.as_slice()) {
                let pattern_index = m.pattern_index;
                // writeln!(&mut stdout, "pattern: {pattern_index}")?;
                for capture in m.captures {
                    let capture_index = capture.index;
                    let capture_name = &query.capture_names()[capture_index as usize];
                    let Point {
                        row: start_row,
                        column: start_column,
                    } = capture.node.start_position();
                    let Point {
                        row: end_row,
                        column: end_column,
                    } = capture.node.end_position();
                    let pos_c = if start_row != last_row {
                        last_row = start_row;
                        c.pos1
                    } else {
                        c.pos2
                    };
                    let pos = format!("{start_row}:{start_column:<3} - {end_row}:{end_column}");
                    if end_row == start_row {
                        let capture_text = capture.node.utf8_text(&source_code).unwrap_or("");
                        writeln!(
                            &mut stdout,
                            // "  {pos:<15} capture: {capture_index:2} - {capture_name}, text: `{capture_text}`"
                            // "{pos:<15} {pattern_index:>2}:{capture_index:<2} {capture_name:>20} `{capture_text}`"
                            "{P}{pos:<18} {PI}{pattern_index:>3}{CL}:{CI}{capture_index:<3} {CN}{capture_name:<max_capture_len$} {BK}`{CT}{capture_text}{BK}`{R}",
                            P=pos_c.prefix(), PI=c.field.prefix(), CL=c.text.prefix(), CI=c.nonterm.prefix(),
                            CN=c.bytes.prefix(), CT=c.text.prefix(), BK=c.backtick.prefix(), R=c.backtick.suffix(),
                        )?;
                    } else {
                        writeln!(&mut stdout, "    {pos:<15} {capture_name}")?;
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
    }

    Ok(())
}
