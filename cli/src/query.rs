use crate::query_testing;
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

    let mut query_cursor = QueryCursor::new();
    if let Some(range) = range {
        query_cursor.set_byte_range(range);
    }

    let mut parser = Parser::new();
    parser.set_language(language)?;

    for path in paths {
        let mut results = Vec::new();

        writeln!(&mut stdout, "{}", path.to_string_lossy())?;

        let source_code =
            fs::read(&path).with_context(|| format!("Error reading source file {:?}", path))?;
        let tree = parser.parse(&source_code, None).unwrap();

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
                let pos = format!("{start_row:3}:{start_column:<2} - {end_row}:{end_column:<3}");
                let capture_text = capture.node.utf8_text(&source_code).unwrap_or("");
                writeln!(
                    &mut stdout,
                    "    {pos:<15} pattern: {pattern_index:>2}, capture: {capture_index:<2} - {capture_name}, text: `{capture_text}`"
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
                writeln!(&mut stdout, "  pattern: {}", pattern_index)?;
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
                    let pos = format!("{start_row:3}:{start_column} - {end_row}:{end_column:<3}");
                    if end_row == start_row {
                        let capture_text = capture.node.utf8_text(&source_code).unwrap_or("");
                        writeln!(
                            &mut stdout,
                            "    {pos:<15} capture: {capture_index:2} - {capture_name}, text: `{capture_text}`")?;
                    } else {
                        writeln!(&mut stdout, "    {pos:<15} capture: {capture_name}",)?;
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
