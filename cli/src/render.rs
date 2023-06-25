use crate::visitor::{Context, Result, Visitor};
use ansi_term::{Color, Style};
use anyhow::bail;
use std::{
    collections::HashSet,
    fmt::Write as _,
    io::{BufRead, Write},
    str::Chars,
};
use tree_sitter::{Node, Point, Range, Tree, TreeCursor};

// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ExtraFlags {
    pub render_timing: bool,
    line_flush: bool,
}

impl Default for ExtraFlags {
    fn default() -> Self {
        Self {
            render_timing: false,
            line_flush: true,
        }
    }
}

impl ExtraFlags {
    fn match_flag(&mut self, flag: char) -> bool {
        match flag {
            '/' => self.render_timing = true,
            'L' => self.line_flush = false,
            'l' => self.line_flush = true,
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Default, Debug)]
pub struct TextFlags {
    pub show: bool,
    pub lines_count_from_one: bool,
}

impl TextFlags {
    fn match_flag(&mut self, flag: char) -> bool {
        match flag {
            '0' => self.lines_count_from_one = false,
            '1' => self.lines_count_from_one = true,
            'T' => self.show = false,
            't' => self.show = true,
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct SExpressionFlags {
    pub text: TextFlags,
    pub extra: ExtraFlags,
    show_positions: bool,
    one_line: bool,
}

impl Default for SExpressionFlags {
    fn default() -> Self {
        Self {
            text: Default::default(),
            extra: Default::default(),
            show_positions: true,
            one_line: false,
        }
    }
}

impl SExpressionFlags {
    fn match_flag(&mut self, flag: char) -> bool {
        match flag {
            'O' => self.one_line = false,
            'o' => self.one_line = true,
            'P' => self.show_positions = false,
            'p' => self.show_positions = true,
            _ => return false,
        }
        true
    }

    pub fn parse(flags: Option<&str>) -> anyhow::Result<Self> {
        let mut f = Self::default();
        if let Some(flags) = flags {
            for ch in flags.chars() {
                if !(f.match_flag(ch) || f.text.match_flag(ch) || f.extra.match_flag(ch)) {
                    bail!("Unknown S-Expression output flag: {ch}");
                }
            }
        }
        Ok(f)
    }
}

#[derive(Clone, Debug)]
pub struct CstFlags {
    pub text: TextFlags,
    pub extra: ExtraFlags,
    show_positions: bool,
    show_byte_positions: bool,
    unquoted_anonymous: bool,
    always_show_full_error_captures: bool,
}

impl Default for CstFlags {
    fn default() -> Self {
        Self {
            text: Default::default(),
            extra: Default::default(),
            show_positions: true,
            show_byte_positions: false,
            unquoted_anonymous: false,
            always_show_full_error_captures: false,
        }
    }
}

impl CstFlags {
    fn match_flag(&mut self, flag: char) -> bool {
        match flag {
            'E' => self.always_show_full_error_captures = false,
            'e' => self.always_show_full_error_captures = true,
            'U' => self.unquoted_anonymous = false,
            'u' => self.unquoted_anonymous = true,
            'B' => self.show_byte_positions = false,
            'b' => self.show_byte_positions = true,
            'P' => self.show_positions = false,
            'p' => self.show_positions = true,
            _ => return false,
        }
        true
    }

    pub fn parse(flags: Option<&str>) -> anyhow::Result<Self> {
        let mut f = Self::default();
        if let Some(flags) = flags {
            for ch in flags.chars() {
                if !(f.match_flag(ch) || f.text.match_flag(ch) || f.extra.match_flag(ch)) {
                    bail!("Unknown CST output flag: {ch}");
                }
            }
        }
        Ok(f)
    }
}

#[derive(Debug, Clone)]
pub enum ScopeRange {
    Range { start: Point, end: Point },
    Node { start: Point },
    ErrorPath,
    Error,
}

impl ScopeRange {
    pub fn parse_inputs(inputs: &[Vec<&str>]) -> anyhow::Result<Vec<Self>> {
        let mut ranges = inputs.iter();
        let mut limit_ranges = Vec::with_capacity(inputs.len().saturating_div(2));
        while let Some(input) = ranges.next() {
            let mut points = input.iter();
            let start = points.next().unwrap();
            let limit_range = match *start {
                "error" => {
                    if input.len() == 1 && inputs.len() == 1 {
                        limit_ranges.push(ScopeRange::Error);
                        return Ok(limit_ranges);
                    } else {
                        bail!("The `--limit-range error` can only be used standalone");
                    }
                }
                "error-path" => {
                    if input.len() == 1 && inputs.len() == 1 {
                        limit_ranges.push(ScopeRange::ErrorPath);
                        return Ok(limit_ranges);
                    } else {
                        bail!("The `--limit-range error-path` can only be used standalone");
                    }
                }
                start => {
                    if input.len() == 1 {
                        match start.ends_with("-") {
                            true => {
                                let start = &start[..start.len().saturating_sub(1)];
                                if start.ends_with("@") {
                                    bail!(
                                        "It's not allowed to use `-` and `@` on a point: {start}"
                                    );
                                }
                                if let Some((start_row, start_column)) = start.split_once(':') {
                                    ScopeRange::Range {
                                        start: Point::new(
                                            start_row.parse()?,
                                            start_column.parse()?,
                                        ),
                                        end: Point::new(usize::MAX, usize::MAX),
                                    }
                                } else {
                                    ScopeRange::Range {
                                        start: Point::new(start.parse()?, 0),
                                        end: Point::new(usize::MAX, usize::MAX),
                                    }
                                }
                            }
                            false => match start.ends_with("@") {
                                false => {
                                    if let Some((start_row, start_column)) = start.split_once(':') {
                                        ScopeRange::Range {
                                            start: Point::default(),
                                            end: Point::new(
                                                start_row.parse()?,
                                                start_column.parse()?,
                                            ),
                                        }
                                    } else {
                                        ScopeRange::Range {
                                            start: Point::default(),
                                            end: Point::new(start.parse()?, 0),
                                        }
                                    }
                                }
                                true => {
                                    let start = &start[..start.len().saturating_sub(1)];
                                    if start.ends_with("-") {
                                        bail!(
                                            "It's not allowed to use `-` and `@` on a point: {start}"
                                        );
                                    }
                                    if let Some((start_row, start_column)) = start.split_once(':') {
                                        ScopeRange::Node {
                                            start: Point::new(
                                                start_row.parse()?,
                                                start_column.parse()?,
                                            ),
                                        }
                                    } else {
                                        ScopeRange::Node {
                                            start: Point::new(start.parse()?, 0),
                                        }
                                    }
                                }
                            },
                        }
                    } else {
                        let end = points.next().unwrap();
                        match (start.split_once(":"), end.split_once(":")) {
                            (None, None) => ScopeRange::Range {
                                start: Point::new(start.parse()?, 0),
                                end: Point::new(end.parse()?, 0),
                            },
                            (None, Some((end_row, end_column))) => ScopeRange::Range {
                                start: Point::new(start.parse()?, 0),
                                end: Point::new(end_row.parse()?, end_column.parse()?),
                            },
                            (Some((start_row, start_column)), None) => ScopeRange::Range {
                                start: Point::new(start_row.parse()?, start_column.parse()?),
                                end: Point::new(end.parse()?, 0),
                            },
                            (Some((start_row, start_column)), Some((end_row, end_column))) => {
                                ScopeRange::Range {
                                    start: Point::new(start_row.parse()?, start_column.parse()?),
                                    end: Point::new(end_row.parse()?, end_column.parse()?),
                                }
                            }
                        }
                    }
                }
            };
            limit_ranges.push(limit_range);
        }
        limit_ranges.sort_by(|a, b| {
            use ScopeRange::*;
            match (a, b) {
                (Range { start: a, .. }, Range { start: b, .. })
                | (Range { start: a, .. }, Node { start: b })
                | (Node { start: a }, Range { start: b, .. })
                | (Node { start: a }, Node { start: b }) => b.cmp(&a),
                _ => unreachable!("Sorting of `error` or `error-path` cases shouldn't happen"),
            }
        });
        Ok(limit_ranges)
    }
}

// ------------------------------------------------------------------------------------------------

pub struct SExpressionRenderer<'a, W: Write> {
    stdout: W,
    indent_level: usize,
    flags: &'a SExpressionFlags,
}

impl<W: Write> Visitor for SExpressionRenderer<'_, W> {
    #[inline(always)]
    fn on_child(&mut self, _: &mut Context) -> Result {
        self.indent_level += 1;
        Ok(())
    }

    #[inline(always)]
    fn on_parent(&mut self, _: &mut Context) -> Result {
        self.indent_level -= 1;
        Ok(())
    }

    #[inline(always)]
    fn on_root(&mut self, context: &mut Context) -> Result {
        self.node(context)
    }

    #[inline(always)]
    fn on_visit(&mut self, context: &mut Context) -> Result {
        if context.node().is_named() || self.show_all() {
            if context.traversed() {
                self.close_brace()?;
            } else {
                self.lf()?;
                self.indent()?;
                self.node(context)?;
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn on_end(&mut self, _: &mut Context) -> Result {
        self.stdout.write_all(b"\n")?;
        self.stdout.flush()?;
        Ok(())
    }
}

impl<'a, W: Write> SExpressionRenderer<'a, W> {
    pub fn new(stdout: W, flags: &'a SExpressionFlags) -> Self {
        Self {
            stdout,
            indent_level: 0,
            flags,
        }
    }

    #[inline(always)]
    fn indent(&mut self) -> Result {
        if self.flags.one_line {
            self.stdout.write_all(b" ")?;
        } else {
            self.stdout.write_all(&b"  ".repeat(self.indent_level))?;
        }
        Ok(())
    }

    #[inline(always)]
    fn node(&mut self, context: &Context) -> Result {
        if let Some(field_name) = context.field_name() {
            write!(self.stdout, "{}: ", field_name)?;
        }
        let node = context.node();
        if self.flags.show_positions {
            let start = node.start_position();
            let end = node.end_position();
            write!(
                self.stdout,
                "({} [{}, {}] - [{}, {}]",
                node.kind(),
                start.row,
                start.column,
                end.row,
                end.column
            )?;
        } else {
            write!(self.stdout, "({}", node.kind())?;
        }
        Ok(())
    }

    #[inline(always)]
    fn close_brace(&mut self) -> Result {
        self.stdout.write_all(b")")?;
        Ok(())
    }

    #[inline(always)]
    fn lf(&mut self) -> Result {
        if !self.flags.one_line {
            self.stdout.write_all(b"\n")?;
            if self.flags.extra.line_flush {
                self.stdout.flush()?;
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn show_all(&self) -> bool {
        false
    }
}

// ------------------------------------------------------------------------------------------------

pub struct CstRenderer<'a, W: Write> {
    stdout: &'a mut W,
    color: Colors,
    text: &'a [u8],
    indent: usize,
    indent_base: usize,
    indent_level: usize,
    indent_shift: usize,
    last_line_no: usize,
    original_nodes: &'a Option<HashSet<usize>>,
    changed_ranges: &'a Option<Vec<Range>>,
    limit_ranges: Option<Vec<ScopeRange>>,
    flags: &'a CstFlags,
    encoding: Encoding,
    buf: String,
}

impl<'a, W: Write> CstRenderer<'a, W> {
    pub fn new(writer: &'a mut W, text: &'a [u8], flags: &'a CstFlags) -> Self {
        Self {
            color: Colors::new(),
            stdout: writer,
            text,
            indent: 0,
            indent_base: 0,
            indent_level: 0,
            indent_shift: 0,
            last_line_no: usize::MAX,
            original_nodes: &None,
            changed_ranges: &None,
            limit_ranges: None,
            flags,
            encoding: Encoding::UTF8,
            buf: String::with_capacity(1024),
        }
    }

    pub fn encoding(mut self, value: Encoding) -> Self {
        self.encoding = value;
        self
    }

    pub fn original_nodes(mut self, original_nodes: &'a Option<HashSet<usize>>) -> Self {
        self.original_nodes = original_nodes;
        self
    }

    pub fn changed_ranges(mut self, ranges: &'a Option<Vec<Range>>) -> Self {
        self.changed_ranges = ranges;
        self
    }

    pub fn limit_ranges(mut self, ranges: &'a Option<Vec<ScopeRange>>) -> Self {
        self.limit_ranges = ranges.clone();
        self
    }
}

macro_rules! colors {
    ($($name:ident $R: literal $G: literal $B: literal $style:ident)+) => {
        pub struct Colors {
             $(pub $name: Style),+
        }

        impl Colors {
            pub fn new() -> Self {
                Self {
                    $($name: Color::RGB($R, $G, $B).$style(),)+
                }
            }
        }

    };
}

colors! {
    lf       166 172 181 normal
    pos1     188 218 120 normal
    pos2     92  108 115 normal
    bytes    175 122 197 normal
    term     219 219 173 normal
    field    177 220 253 normal
    nonterm  117 187 253 normal
    extra    153 153 255 normal
    text     118 118 118 normal
    edit     255 255 102 normal
    // changed  0   0   255 normal
    renewed  0   255 0   normal
    backtick 101 192 67  normal
    missing  255 153 51  bold
    error    255 51  51  bold
}

impl<W: Write> Visitor for CstRenderer<'_, W> {
    #[inline(always)]
    fn on_root(&mut self, context: &mut Context) -> Result {
        let node = context.node();

        // self.write(format!("root: {}\n\n", node.kind()).as_bytes())?;

        let (modified, error) = self.node_mods(&node);
        if modified {
            self.indent_shift += 1;
        }
        if error {
            self.indent_shift += 1;
        }
        self.on_visit(context)
    }

    #[inline(always)]
    fn on_end(&mut self, _: &mut Context) -> Result {
        self.stdout.flush()?;
        Ok(())
    }

    #[inline(always)]
    fn on_child(&mut self, _: &mut Context) -> Result {
        self.indent_level += 1;
        Ok(())
    }

    #[inline(always)]
    fn on_parent(&mut self, _: &mut Context) -> Result {
        self.indent_level -= 1;
        Ok(())
    }

    #[inline(always)]
    fn on_visit(&mut self, context: &mut Context) -> Result {
        let node = context.node();
        if node.is_named() || self.show_all() {
            if !context.traversed() {
                let check = NodeRangeCheck::check(&mut self.limit_ranges, &node)?;
                if check.draw_extra_lf {
                    self.lf()?;
                }
                if check.hide_row {
                    return Ok(());
                }

                self.indent(&context)?;
                self.node(&context)?;
                self.lf()?;
            }
        }
        Ok(())
    }
}

pub struct NodeRangeCheck {
    pub hide_row: bool,
    pub draw_extra_lf: bool,
}

impl NodeRangeCheck {
    #[inline(always)]
    pub fn check(limit_ranges: &mut Option<Vec<ScopeRange>>, node: &Node) -> anyhow::Result<Self> {
        // Implement a range display logic
        let mut pop = false;
        let mut hide_row = false;
        let mut draw_extra_lf = false;
        if let Some(ranges) = limit_ranges {
            if ranges.is_empty() {
                hide_row = true;
            } else {
                let node_start = node.start_position();
                // dbg!(&ranges, &tail_one);
                if let Some((last, ranges)) = ranges.split_last_mut() {
                    if let ScopeRange::Node { start } = last {
                        if node_start >= *start {
                            *last = ScopeRange::Range {
                                start: *start,
                                end: node.end_position(),
                            };
                        }
                    };

                    let (range_start, range_end) = match last {
                        ScopeRange::Range { start, end } => (&*start, &*end),
                        ScopeRange::Node { start } => (&*start, &*start),
                        ScopeRange::ErrorPath => todo!(),
                        ScopeRange::Error => todo!(),
                    };

                    if node_start < *range_start || node_start >= *range_end {
                        hide_row = true;
                    }
                    if node_start >= *range_end {
                        pop = true;
                        if !ranges.is_empty() {
                            draw_extra_lf = true;
                        }
                        if let Some(range) = ranges.last() {
                            let range_start = match range {
                                ScopeRange::Range { start, .. } => start,
                                ScopeRange::Node { start } => start,
                                ScopeRange::ErrorPath => todo!(),
                                ScopeRange::Error => todo!(),
                            };

                            if node_start < *range_start {
                                hide_row = true;
                            }
                        }
                    }
                }
            }
            if pop {
                ranges.pop();
            }
        }

        Ok(Self {
            hide_row,
            draw_extra_lf,
        })
    }

    pub fn check_parent_scoped(
        tree_cursor: &mut TreeCursor,
        limit_ranges: &mut Option<Vec<ScopeRange>>,
        node: &Node,
    ) -> anyhow::Result<Self> {
        fn goto_node_for_point<'a>(cursor: &'a mut TreeCursor, point: &Point) -> Option<Node<'a>> {
            loop {
                if cursor.node().start_position() < *point {
                    break;
                }
                if !cursor.goto_parent() {
                    return None; // exit if cursor is scoped to a node and can't go further
                };
            }
            let node = loop {
                let node = cursor.node();
                if *point > node.start_position() {
                    if let Some(sibling) = node.next_sibling() {
                        if *point < sibling.start_position() {
                            if !cursor.goto_first_child() {
                                break sibling;
                            }
                        } else {
                            cursor.goto_next_sibling();
                        }
                    } else if !cursor.goto_first_child() {
                        break node;
                    }
                } else {
                    break node;
                }
            };
            Some(node)
        }

        if let Some(ranges) = limit_ranges {
            if let Some((last, _)) = ranges.split_last_mut() {
                if let ScopeRange::Node { start } = last {
                    if let Some(node) = goto_node_for_point(tree_cursor, start) {
                        *last = ScopeRange::Range {
                            start: node.start_position(),
                            end: node.end_position(),
                        };
                    }
                };
            }
        }
        Self::check(limit_ranges, node)
    }
}

const NODE_PAD: &str = " ";
const MULTILINE_PAD: &str = " ";

impl<'a, W: Write> CstRenderer<'a, W> {
    #[inline(always)]
    fn indent(&mut self, context: &Context) -> Result {
        self.indent_base = self.indent_shift + self.indent_level * 2;
        self.indent_base += self.flags.show_byte_positions.then(|| 20).unwrap_or(15); // TODO: Implement a waterline idea
        self.indent = self.indent_base;
        let node = context.node();

        let (modified, error) = self.node_mods(&node);
        if modified {
            self.indent -= 1;
        }
        if error {
            self.indent -= 1;
        }

        if self.flags.show_positions {
            let Point {
                row: start_row,
                column: start_column,
            } = node.start_position();
            let Point {
                row: end_row,
                column: end_column,
            } = node.end_position();

            let pos_color = {
                if self.last_line_no != start_row {
                    self.color.pos1
                } else {
                    self.color.pos2
                }
            };
            let pos = format!("{start_row}:{start_column:<2} - {end_row}:{end_column:<2}");
            let mut pos_len = pos.len();
            write!(
                self.stdout,
                "{C}{pos}{R}",
                C = pos_color.prefix(),
                R = pos_color.suffix(),
            )?;

            if self.flags.show_byte_positions {
                let byte_pos = format!("{:4}:{:<2}", node.start_byte(), node.end_byte());
                pos_len += byte_pos.len();
                write!(
                    self.stdout,
                    "{C}{byte_pos}{R}",
                    C = self.color.bytes.prefix(),
                    R = self.color.bytes.suffix()
                )?;
            }

            let indent = self.indent.checked_sub(pos_len).unwrap_or(1);
            write!(self.stdout, "{}", NODE_PAD.repeat(indent))?;
            self.render_dot_marks(&node)?;

            self.last_line_no = start_row;
        } else {
            self.write(&*NODE_PAD.repeat(self.indent).as_bytes())?;
            self.render_dot_marks(&node)?;
        }
        Ok(())
    }

    #[inline(always)]
    fn render_dot_marks(&mut self, node: &Node) -> Result {
        if node.has_error() || node.is_error() {
            self.write_colored("•", self.color.error)?;
        }
        if node.has_changes() {
            self.write_colored("•", self.color.edit)?;
        } else if let Some(map) = self.original_nodes {
            if !map.contains(&node.id()) {
                self.write_colored("•", self.color.renewed)?;
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn is_new_node(&self, node: &Node) -> bool {
        if let Some(map) = self.original_nodes {
            return !map.contains(&node.id());
        }
        false
    }

    #[inline(always)]
    fn node_mods(&self, node: &Node) -> (bool, bool) {
        let has_changes = node.has_changes();
        let is_new_node = self.is_new_node(node);
        let is_missing = node.is_missing();
        let has_error = node.has_error();
        let is_error = node.is_error();

        let modified = has_changes || is_new_node;
        let error = has_error || is_error || is_missing;
        (modified, error)
    }

    #[inline(always)]
    fn node(&mut self, context: &Context) -> Result {
        let node = context.node();
        let node_color = if node.is_error() {
            self.color.error
        } else if node.is_extra() {
            self.color.extra
        } else if node.is_named() {
            self.color.nonterm
        } else {
            self.color.term
        };
        if node.is_missing() {
            self.write_colored("MISSING: ", self.color.missing)?;
        }
        if let Some(field_name) = context.field_name() {
            write!(self.stdout, "{}: ", self.color.field.paint(field_name),)?;
        }
        if node.is_named() {
            self.write_colored(node.kind(), node_color)?;

            if node.child_count() == 0
                || (node.is_error() && self.flags.always_show_full_error_captures)
            {
                let start = node.start_byte();
                let end = node.end_byte();
                // Don't show for MISSING empty tokens
                if end > start {
                    let slice = &self.text[start..end];

                    let mut value = match self.encoding {
                        Encoding::UTF8 => std::str::from_utf8(slice)?,
                        Encoding::UTF16LE => {
                            let slice = as_u16_slice(slice);
                            self.buf.clear();
                            let chars = char::decode_utf16(slice.iter().map(|x| x.to_le()));
                            for ch in chars {
                                self.buf.push(ch?);
                            }
                            unsafe { &*(&*self.buf as *const _) }
                        }
                        Encoding::UTF16BE => {
                            let slice = as_u16_slice(slice);
                            self.buf.clear();
                            let chars = char::decode_utf16(slice.iter().map(|x| x.to_be()));
                            for ch in chars {
                                self.buf.push(ch?);
                            }
                            unsafe { &*(&*self.buf as *const _) }
                        }
                    };

                    if node.kind() != value || node.is_named() {
                        let mut multiline = false;
                        let mut row = node.start_position().row;
                        let mut pos_color = self.color.pos2;
                        let mut pos = String::with_capacity(32); // TODO: Implement without this allocation
                        loop {
                            let v;
                            if let Some(idx) = value.find('\n') {
                                if idx + 1 == value.len() {
                                    v = value;
                                } else {
                                    v = &value[..idx + 1];
                                    value = &value[idx + 1..];
                                }
                            } else {
                                v = value;
                            }
                            pos.clear();
                            let mut p = self.indent_base;
                            if self.flags.show_positions {
                                let col = if multiline {
                                    0
                                } else {
                                    node.start_position().column
                                };
                                write!(&mut pos, "{}:{:<2} - {}:{}", row, col, row, v.len())?;
                                p -= pos.len();
                            };
                            if &v != &value {
                                multiline = true;
                                write!(self.stdout, "\n")?;
                                self.write_colored(&*pos, pos_color)?;
                                self.render_node_text(v, p + 2)?;
                                row += 1;
                            } else {
                                if multiline {
                                    write!(self.stdout, "\n")?;
                                    self.write_colored(&*pos, pos_color)?;
                                    self.render_node_text(v, p + 2)?;
                                } else {
                                    self.render_node_text(v, 1)?;
                                };

                                break;
                            }
                            pos_color = self.color.pos1;
                        }
                    }
                }
            }
        } else {
            if node.is_error() {
                self.write_colored("ERROR: ", self.color.error)?;
            }
            let s = escape_chars(node.kind()).collect();
            let s = if self.flags.unquoted_anonymous {
                s
            } else {
                format!("\"{}\"", s)
            };
            self.write_colored(&*s, node_color)?;
        }

        // self.write(
        //     format!(
        //         // " -- {}, {}:{}",
        //         // self.indent_shift,
        //         " -- {}:{}",
        //         node.start_byte(),
        //         node.end_byte()
        //     )
        //     .as_bytes(),
        // )?;
        Ok(())
    }

    #[inline(always)]
    fn render_node_text(&mut self, value: &str, pad_size: usize) -> Result {
        let pad = MULTILINE_PAD.repeat(pad_size);
        let r = {
            if pad_size > 0 && value.ends_with('\n') {
                let value = &value[..value.len() - 1];
                write!(
                    self.stdout,
                    "{pad}{}`{}{}\\n{}`{}",
                    self.color.backtick.prefix(),
                    self.color
                        .text
                        .paint(escape_invisible_symbols(value).collect::<String>()),
                    self.color.lf.prefix(),
                    self.color.backtick.prefix(),
                    self.color.backtick.suffix()
                )
            } else {
                write!(
                    self.stdout,
                    "{pad}{}`{}{}`{}",
                    self.color.backtick.prefix(),
                    self.color
                        .text
                        .paint(escape_invisible_symbols(value).collect::<String>()),
                    self.color.backtick.prefix(),
                    self.color.backtick.suffix()
                )
            }
        };
        Ok(r?)
    }

    #[inline(always)]
    fn lf(&mut self) -> Result {
        self.write(b"\n")?;
        Ok(())
    }

    #[inline(always)]
    fn show_all(&self) -> bool {
        true
    }
}

impl<'a, W: Write> CstRenderer<'a, W> {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> Result {
        Ok(self.stdout.write_all(buf)?)
    }

    #[inline(always)]
    fn write_colored(&mut self, buf: &str, color: Style) -> Result {
        self.write(color.paint(buf).to_string().as_bytes())
    }
}

#[inline(always)]
pub fn escape_chars(s: &str) -> impl Iterator<Item = char> + '_ {
    translate_symbols(s, escape_char)
}

#[inline(always)]
fn escape_char(c: char) -> Option<&'static str> {
    Some(match c {
        '\\' => "\\\\",
        '\"' => "\\\"",
        _ => return escape_invisible(c),
    })
}

#[inline(always)]
pub fn escape_invisible_symbols(s: &str) -> impl Iterator<Item = char> + '_ {
    translate_symbols(s, escape_invisible)
}

#[inline(always)]
fn escape_invisible(c: char) -> Option<&'static str> {
    Some(match c {
        '\n' => "\\n",
        '\r' => "\\r",
        '\t' => "\\t",
        '\0' => "\\0",
        '\\' => "\\\\",
        '\x0b' => "\\v",
        '\x0c' => "\\f",
        _ => return None,
    })
}

#[inline(always)]
pub fn translate_symbols<'s, F>(s: &'s str, escape_fn: F) -> impl Iterator<Item = char> + 's
where
    F: Fn(char) -> Option<&'static str> + 's,
{
    struct Escape<'s, F> {
        chars: Chars<'s>,
        sub: Option<Chars<'s>>,
        escape_fn: F,
    }
    impl<'s, F> Escape<'s, F> {
        fn sub(&mut self, sub: &'s str) -> Option<char> {
            let mut sub = sub.chars();
            let c = sub.next();
            self.sub = Some(sub);
            c
        }
    }
    impl<F> Iterator for Escape<'_, F>
    where
        F: Fn(char) -> Option<&'static str>,
    {
        type Item = char;
        fn next(&mut self) -> Option<Self::Item> {
            if let Some(sub) = &mut self.sub {
                if let Some(c) = sub.next() {
                    return Some(c);
                } else {
                    self.sub = None
                }
            }
            if let Some(c) = self.chars.next() {
                match (self.escape_fn)(c) {
                    Some(sub) => self.sub(sub),
                    None => Some(c),
                }
            } else {
                None
            }
        }
    }
    Escape {
        chars: s.chars(),
        sub: None,
        escape_fn,
    }
}

pub fn collect_node_ids(tree: &mut Tree) -> HashSet<usize> {
    let mut cursor = tree.walk();
    let mut node_ids = HashSet::new();
    let mut visit = |cursor: &TreeCursor| {
        let node = cursor.node();
        if let false = node_ids.insert(node.id()) {
            let start = node.start_position();
            let end = node.end_position();
            panic!(
                "Node id exists: {} {}:{:<2} - {}:{:<2} {}",
                node.id(),
                start.row,
                start.column,
                end.row,
                end.column,
                node.kind(),
            );
        }
    };
    let mut visited = false;
    loop {
        if !visited {
            visit(&cursor);
        }
        // Traverse logic --------------
        if !visited && cursor.goto_first_child() {
            visited = false;
        } else if cursor.goto_next_sibling() {
            visited = false;
        } else if cursor.goto_parent() {
            visited = true;
        } else {
            break;
        }
        //------------------------------
    }
    node_ids
}

// ------------------------------------------------------------------------------------------------

pub fn xml_render(stdout: &mut impl Write, cursor: &mut TreeCursor, text: &[u8]) -> Result {
    let mut needs_newline = false;
    let mut indent_level = 0;
    let mut did_visit_children = false;
    let mut tags: Vec<&str> = Vec::new();
    let start_node = cursor.node();
    loop {
        let node = cursor.node();
        let is_named = node.is_named();
        if did_visit_children {
            if is_named {
                let tag = tags.pop();
                write!(stdout, "</{}>\n", tag.expect("there is a tag"))?;
                needs_newline = true;
            }
            if cursor.goto_next_sibling() {
                did_visit_children = false;
            } else if cursor.goto_parent() {
                did_visit_children = true;
                indent_level -= 1;
            } else {
                break;
            }
        } else {
            if is_named {
                if needs_newline {
                    stdout.write(b"\n")?;
                }
                for _ in 0..indent_level {
                    stdout.write(b"  ")?;
                }
                write!(stdout, "<{}", node.kind())?;
                if let Some(field_name) = cursor.field_name() {
                    write!(stdout, " type=\"{}\"", field_name)?;
                }
                write!(stdout, ">")?;
                tags.push(node.kind());
                needs_newline = true;
            }
            if cursor.goto_first_child() {
                did_visit_children = false;
                indent_level += 1;
            } else {
                did_visit_children = true;
                let start = node.start_byte();
                let end = node.end_byte();
                let value = std::str::from_utf8(&text[start..end]).expect("has a string");
                write!(stdout, "{}", html_escape::encode_text(value))?;
            }
        }
    }
    cursor.reset(start_node);
    stdout.flush()?;
    println!("");
    Ok(())
}

// ------------------------------------------------------------------------------------------------

pub fn render_text(stdout: &mut impl Write, offset: usize, source_code: &[u8]) -> Result {
    stdout.write_all(b"\n")?;
    let n_color = Color::Blue.normal();
    for (mut i, s) in BufRead::split(source_code, b'\n').enumerate() {
        i += offset;
        write!(stdout, "{}{i:<2}{} ", n_color.prefix(), n_color.suffix())?;
        stdout.write_all(&*s.unwrap())?;
        stdout.write_all(b"\n")?;
    }
    stdout.flush()?;
    Ok(())
}

// ------------------------------------------------------------------------------------------------

pub fn render_changed_ranges(stdout: &mut impl Write, changed_ranges: &[Range]) -> Result {
    let c = crate::render::Colors::new();
    writeln!(stdout)?;
    // println!(
    //     "\n{C}Changed ranges:{R}",
    //     C = c.field.prefix(),
    //     R = c.field.suffix()
    // );
    for range in changed_ranges {
        let Range {
            start_byte,
            end_byte,
            start_point:
                Point {
                    row: start_row,
                    column: start_column,
                },
            end_point:
                Point {
                    row: end_row,
                    column: end_column,
                },
        } = range;
        writeln!(stdout,
            "{P}{start_row}:{start_column:<2} - {end_row}:{end_column:<2} {B}{start_byte:3}:{end_byte}{R}",
            P=c.term.prefix(), B=c.bytes.prefix(), R=c.nonterm.suffix()
        )?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum Encoding {
    UTF8,
    UTF16LE,
    UTF16BE,
}

impl Encoding {
    pub fn bom(&self) -> &'static [u8] {
        match self {
            Encoding::UTF8 => &[0xEF, 0xBB, 0xBF],
            Encoding::UTF16LE => &[0xFF, 0xFE],
            Encoding::UTF16BE => &[0xFE, 0xFF],
        }
    }

    pub fn test_bytes(input: &[u8]) -> Option<Self> {
        for enc in [Self::UTF8, Self::UTF16LE, Self::UTF16BE] {
            if input.len() >= enc.bom().len() {
                if &input[..enc.bom().len()] == enc.bom() {
                    return Some(enc);
                }
            }
        }

        None
    }
}

pub fn as_u16_slice(slice: &[u8]) -> &[u16] {
    assert!(slice.len() % 2 == 0);
    let len = slice.len() / 2;
    let ptr = slice.as_ptr().cast::<u16>();
    unsafe { std::slice::from_raw_parts(ptr, len) }
}
