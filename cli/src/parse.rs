use super::util;
use crate::input::ParserInput;
use crate::render::{
    as_u16_slice, collect_node_ids, render_changed_ranges, render_text, xml_render, CstFlags,
    CstRenderer, Encoding, SExpressionFlags, SExpressionRenderer, ScopeRange,
};
use crate::visitor::Visitor;
use ansi_term::Color;
use anyhow::{anyhow, bail, Result};
use std::io::{self, Write};
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};
use std::{fmt, thread, usize};
use tree_sitter::{InputEdit, LogType, Parser, Point, Tree};

#[derive(Clone, Debug)]
pub enum OutputFormat {
    SExpression(SExpressionFlags),
    Cst(CstFlags),
    Xml,
}

impl OutputFormat {
    pub fn parse(format: &str) -> Result<Self> {
        let (format, flags) = match format.split_once(':') {
            Some((format, flags)) => (format, Some(flags)),
            None => (format, None),
        };
        Ok(match format {
            "s" | "s-expression" => Self::SExpression(SExpressionFlags::parse(flags)?),
            "c" | "cst" => Self::Cst(CstFlags::parse(flags)?),
            "x" | "xml" => {
                if flags.is_some() {
                    bail!("XML output format doesn't support flags");
                }
                Self::Xml
            }
            format => {
                if format.len() > 1 {
                    let mut format = format.to_owned();
                    let prefixes = ["s-expression", "cst", "xml"];
                    if prefixes.iter().any(|s| format.starts_with(s)) {
                        bail!("Flags should be separated by a colon: `:`")
                    }
                    format.insert(1, ':');
                    Self::parse(format.as_str())?
                } else {
                    bail!("Unknown output format: {format}")
                }
            }
        })
    }
}

#[derive(Debug)]
pub struct Edit {
    pub position: usize,
    pub deleted_length: usize,
    pub inserted_text: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub successful_parses: usize,
    pub total_parses: usize,
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return writeln!(f, "Total parses: {}; successful parses: {}; failed parses: {}; success percentage: {:.2}%",
                 self.total_parses,
                 self.successful_parses,
                 self.total_parses - self.successful_parses,
                 (self.successful_parses as f64) / (self.total_parses as f64) * 100.0);
    }
}

pub fn parse_input(
    mut input: ParserInput,
    output: Option<&OutputFormat>,
    output_dot: bool,
    edits: Option<&[&str]>,
    apply_edits: bool,
    limit_ranges: &Option<Vec<Vec<&str>>>,
    print_time: bool,
    quiet: bool,
    debug: bool,
    debug_graph: bool,
    timeout: u64,
    cancellation_flag: Option<&AtomicUsize>,
    max_path_length: usize,
    show_file_names: usize,
) -> Result<bool> {
    let mut parser = Parser::new();
    parser.set_language(input.language)?;

    // If the `--cancel` flag was passed, then cancel the parse
    // when the user types a newline.
    unsafe { parser.set_cancellation_flag(cancellation_flag) };

    // Set a timeout based on the `--time` flag.
    parser.set_timeout_micros(timeout);
    // Render an HTML graph if `--debug-graph` was passed

    // Log to stderr if `--debug` was passed
    if debug {
        parser.set_logger(Some(Box::new(|log_type, message| {
            if log_type == LogType::Lex {
                io::stderr().write(b"  ").unwrap();
            }
            write!(&mut io::stderr(), "{}\n", message).unwrap();
        })));
    }

    let time = Instant::now();

    let (encoding, bom_len) = Encoding::test_bytes(&input.source_code)
        .map(|x| (x, x.bom().len()))
        .unwrap_or((Encoding::UTF8, 0));

    // let tree = parser.parse(&input.source_code, None);

    let scope = thread::scope(|s| {
        let counts = s.spawn(|| {
            (
                input.source_code.len(),
                bytecount::count(&input.source_code, b'\n'),
            )
        });
        let tree = match encoding {
            Encoding::UTF8 => parser.parse(&input.source_code, None),
            Encoding::UTF16LE => {
                let source_code = as_u16_slice(&input.source_code);
                parser.parse_utf16_le(source_code, None)
            }
            Encoding::UTF16BE => {
                let source_code = as_u16_slice(&input.source_code);
                parser.parse_utf16_be(source_code, None)
            }
        };

        let (bytes_count, lines_count) = counts.join().expect("Can't start a thread");
        (tree, bytes_count, lines_count)
    });
    let (tree, bytes_count, lines_count) = scope;

    let mut stdout = io::stdout();

    if let Some(mut tree) = tree {
        let node_ids = if apply_edits {
            Some(collect_node_ids(&mut tree))
        } else {
            None
        };

        let mut changed_ranges = None;

        if let Some(edits) = edits {
            if debug_graph {
                println!("BEFORE:\n{}", String::from_utf8_lossy(&input.source_code));
            }
            let mut i = 0;
            let mut edits = edits.iter();
            while let Some(position) = edits.next() {
                let deleted_length = edits.next().unwrap();
                let inserted_text = edits.next().unwrap();
                let edit = create_edit(
                    &input.source_code,
                    *position,
                    *deleted_length,
                    *inserted_text,
                )?;
                perform_edit(&mut tree, &mut input.source_code, &edit);
                if debug_graph {
                    i += 1;
                    println!(
                        "AFTER {}:\n{}",
                        i,
                        String::from_utf8_lossy(&input.source_code)
                    );
                }
            }
            let edited_tree = tree.clone();
            if apply_edits {
                tree = parser.parse(&input.source_code, Some(&edited_tree)).unwrap();
                changed_ranges.replace(edited_tree.changed_ranges(&tree).collect());
            }
        }

        let duration = time.elapsed();
        let duration_ms = duration.as_secs() * 1000 + duration.subsec_nanos() as u64 / 1000000;

        let (lines_count_from_one, mut show_text) = match output {
            Some(OutputFormat::SExpression(flags)) => {
                (flags.text.lines_count_from_one, flags.text.show)
            }
            Some(OutputFormat::Cst(flags)) => (flags.text.lines_count_from_one, flags.text.show),
            _ => (false, false),
        };

        let row_offset = lines_count_from_one.then(|| 1).unwrap_or(0);

        let mut cursor = tree
            .root_node_with_offset(
                0,
                Point {
                    row: row_offset,
                    column: 0,
                },
            )
            .walk();

        let mut cst_output = false;
        if !quiet {
            let limit_ranges = limit_ranges
                .as_ref()
                .map(|limit_ranges| ScopeRange::parse_inputs(&limit_ranges))
                .transpose()?;

            fn timeit<T>(mut func: impl FnMut() -> Result<T>) -> Result<(T, Duration)> {
                let time = Instant::now();
                let result = func()?;
                Ok((result, time.elapsed()))
            }
            fn render_timing<T>(mut func: impl FnMut() -> Result<T>, time_it: bool) -> Result<()> {
                if time_it {
                    let (_, duration) = timeit(func)?;
                    eprintln!("\n--- rendered: {:?}", duration);
                } else {
                    func()?;
                }
                Ok(())
            }

            let name_color = Color::RGB(38, 166, 154);

            #[cfg(not(unix))]
            let mut stdout = stdout.lock();
            #[cfg(unix)]
            let mut stdout: fast_stdout::FastStdout = stdout.lock().into();

            match output {
                None => {
                    let flags = SExpressionFlags::default();
                    SExpressionRenderer::new(&mut stdout, &flags).perform(cursor.clone())?;
                }
                Some(OutputFormat::SExpression(flags)) => {
                    let func =
                        || SExpressionRenderer::new(&mut stdout, flags).perform(cursor.clone());
                    render_timing(func, flags.extra.render_timing)?;
                }
                Some(OutputFormat::Cst(flags)) => {
                    cst_output = true;
                    let func = || {
                        CstRenderer::new(&mut stdout, &input.source_code, flags)
                            .original_nodes(&node_ids)
                            .changed_ranges(&changed_ranges)
                            .limit_ranges(&limit_ranges)
                            .source_counts(bytes_count, lines_count)
                            .encoding(encoding)
                            .perform(cursor.clone())
                    };
                    if show_file_names > 0 {
                        println!(
                            "{C}{}{R}",
                            input.origin,
                            C = name_color.prefix(),
                            R = name_color.suffix()
                        )
                    }
                    render_timing(func, flags.extra.render_timing)?;
                    if let Some(ranges) = changed_ranges {
                        render_changed_ranges(&mut stdout, &ranges)?;
                    }
                    if show_text {
                        render_text(&mut stdout, row_offset, &input.source_code[bom_len..])?;
                        show_text = false;
                    }
                    if show_file_names > 1 {
                        println!()
                    }
                }
                Some(OutputFormat::Xml) => {
                    xml_render(&mut stdout, &mut cursor, &input.source_code)?;
                }
            }

            if show_text {
                render_text(&mut stdout, row_offset, &input.source_code[bom_len..])?;
            }
        }

        if output_dot {
            util::print_tree_graph(&tree, "log.html").unwrap();
        }

        let mut stdout = stdout.lock();
        let mut first_error = None;
        loop {
            let node = cursor.node();
            if node.has_error() {
                if node.is_error() || node.is_missing() {
                    first_error = Some(node);
                    break;
                } else {
                    if !cursor.goto_first_child() {
                        break;
                    }
                }
            } else if !cursor.goto_next_sibling() {
                break;
            }
        }

        if (first_error.is_some() || print_time) && !cst_output {
            write!(
                &mut stdout,
                "{:width$}\t{} ms",
                input.origin.as_str(),
                duration_ms,
                width = max_path_length
            )?;
            if let Some(node) = first_error {
                let start = node.start_position();
                let end = node.end_position();
                write!(&mut stdout, "\t(")?;
                if node.is_missing() {
                    if node.is_named() {
                        write!(&mut stdout, "MISSING {}", node.kind())?;
                    } else {
                        write!(
                            &mut stdout,
                            "MISSING \"{}\"",
                            node.kind().replace("\n", "\\n")
                        )?;
                    }
                } else {
                    write!(&mut stdout, "{}", node.kind())?;
                }
                write!(
                    &mut stdout,
                    " [{}, {}] - [{}, {}])",
                    start.row, start.column, end.row, end.column
                )?;
            }
            write!(&mut stdout, "\n")?;
        }

        return Ok(first_error.is_some());
    } else if print_time {
        let duration = time.elapsed();
        let duration_ms = duration.as_secs() * 1000 + duration.subsec_nanos() as u64 / 1000000;
        writeln!(
            &mut stdout,
            "{:width$}\t{} ms (timed out)",
            input.origin.as_str(),
            duration_ms,
            width = max_path_length
        )?;
    }

    Ok(false)
}

pub fn perform_edit(tree: &mut Tree, input: &mut Vec<u8>, edit: &Edit) -> InputEdit {
    let start_byte = edit.position;
    let old_end_byte = edit.position + edit.deleted_length;
    let new_end_byte = edit.position + edit.inserted_text.len();
    let start_position = position_for_offset(input, start_byte);
    let old_end_position = position_for_offset(input, old_end_byte);
    input.splice(start_byte..old_end_byte, edit.inserted_text.iter().cloned());
    let new_end_position = position_for_offset(input, new_end_byte);
    let edit = InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position,
        old_end_position,
        new_end_position,
    };
    tree.edit(&edit);
    edit
}

fn create_edit(
    source_code: &Vec<u8>,
    position: &str,
    deleted_length: &str,
    inserted_text: &str,
) -> Result<Edit> {
    let error = || {
        anyhow!(concat!(
            "Invalid edit: {} {} `{}`. ",
            "Edit strings must match the pattern '<START_BYTE_OR_POSITION> <REMOVED_LENGTH> <NEW_TEXT>'"
        ), position, deleted_length, inserted_text)
    };

    let parts = if position.contains(",") {
        Some(position.split(","))
    } else if position.contains(":") {
        Some(position.split(":"))
    } else {
        None
    };

    // Position can either be a byte_offset or row,column pair, separated by a comma
    let position = {
        if let Some(mut parts) = parts {
            let row = parts.next().ok_or_else(error)?;
            let row = usize::from_str_radix(row, 10).map_err(|_| error())?;
            let column = parts.next().ok_or_else(error)?;
            let column = usize::from_str_radix(column, 10).map_err(|_| error())?;
            offset_for_position(source_code, Point { row, column })
        } else if position == "$" {
            source_code.len()
        } else {
            usize::from_str_radix(position, 10).map_err(|_| error())?
        }
    };

    // Deleted length must be a byte count.
    let deleted_length = usize::from_str_radix(deleted_length, 10).map_err(|_| error())?;

    Ok(Edit {
        position,
        deleted_length,
        inserted_text: unescape_lf(inserted_text.as_bytes()),
    })
}

fn offset_for_position(input: &Vec<u8>, position: Point) -> usize {
    let mut current_position = Point { row: 0, column: 0 };
    for (i, c) in input.iter().enumerate() {
        if *c as char == '\n' {
            current_position.row += 1;
            current_position.column = 0;
        } else {
            current_position.column += 1;
        }
        if current_position > position {
            return i;
        }
    }
    return input.len();
}

fn position_for_offset(input: &Vec<u8>, offset: usize) -> Point {
    let mut result = Point { row: 0, column: 0 };
    for c in &input[0..offset] {
        if *c as char == '\n' {
            result.row += 1;
            result.column = 0;
        } else {
            result.column += 1;
        }
    }
    result
}

#[cfg(not(unix))]
pub fn unescape_lf(buf: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(buf.len());
    let len = buf.len();
    let mut i = 0;
    while i < len {
        let c = buf[i];
        if c as char == '\\' && i < len {
            match buf[i + 1] as char {
                'n' => {
                    out.push('\n' as u8);
                }
                c => {
                    out.push('\\' as u8);
                    out.push(c as u8);
                }
            }
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(unix)]
pub fn unescape_lf(buf: &[u8]) -> Vec<u8> {
    buf.to_vec()
}

#[cfg(unix)]
mod fast_stdout {
    use std::fs::File;
    use std::io::{self, BufWriter, StdoutLock, Write};
    use std::mem::ManuallyDrop;
    use std::os::fd::FromRawFd;

    pub(crate) struct FastStdout<'a> {
        #[allow(dead_code)]
        lock: StdoutLock<'a>,
        stdout: ManuallyDrop<BufWriter<File>>,
    }

    impl<'a> FastStdout<'a> {
        fn new(lock: StdoutLock<'a>) -> Self {
            Self {
                lock,
                // ManuallyDrop requires to don't close fd that required opened for StdoutLock
                stdout: ManuallyDrop::new(BufWriter::new(unsafe { File::from_raw_fd(1) })),
            }
        }
    }

    impl Write for FastStdout<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.stdout.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.stdout.flush()
        }
    }

    impl<'a> From<StdoutLock<'a>> for FastStdout<'a> {
        fn from(value: StdoutLock<'a>) -> Self {
            Self::new(value)
        }
    }
}
