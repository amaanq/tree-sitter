use crate::cli::{Cli, LanguageArgs};
use crate::node_types::NodeTypes;
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use libloading::Library;
use parser_source::ParserSource;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use std::time::SystemTime;
use std::{mem, path::PathBuf};
use tree_sitter::Language;
use tree_sitter_loader::Loader;

mod cli {
    use clap::{Args, Parser};
    use std::path::PathBuf;

    #[derive(Parser)]
    pub(crate) struct Cli {
        #[command(flatten)]
        pub language_args: LanguageArgs,

        /// Convert symbols to uppercase
        #[arg(short, long)]
        pub uppercase: bool,
    }

    #[derive(Args)]
    pub(crate) struct LanguageArgs {
        /// path to a compiled library `*.so`, `*.dylib` or `*.dll` or a language source dir
        pub language_path: PathBuf,

        /// path to a related node-types.json file for a dynamic library
        #[arg(short, long, value_name = "FILE")]
        pub node_types: Option<PathBuf>,
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let LanguageArgs {
        language_path,
        mut node_types,
    } = cli.language_args;

    if language_path.is_dir() {
        verify_times(language_path.as_path(), 3)
            .with_context(|| anyhow!("The grammar needs to be regenerated"))?;
    }

    let language = load_language(Some(language_path.clone()), &mut node_types)?;
    let node_types = node_types
        .map(|path| NodeTypes::from_file(path).ok())
        .flatten();
    let parser_source = Some(language_path)
        .map(|p| p.is_dir().then_some(p))
        .flatten()
        .map(|p| ParserSource::from_file(p.join("src").join("parser.c")))
        .transpose()?;
    info::print_language_info(&language, node_types, cli.uppercase, parser_source.as_ref())?;

    Ok(())
}

fn load_language(path: Option<PathBuf>, node_types: &mut Option<PathBuf>) -> Result<Language> {
    let mut loader;
    let language = if let Some(path) = &path {
        if !path.exists() {
            return Err(anyhow!("Path doesn't exists"));
        }
        if path.is_file() {
            unsafe {
                eprintln!("loading library: {}", path.to_string_lossy());

                let language_name = path.file_stem().unwrap().to_str().unwrap();
                eprintln!("language name: {}", language_name);

                let symbol = format!(
                    "tree_sitter_{}",
                    identifier::replace_dashes_with_underscores(language_name)
                )
                .to_string();
                eprintln!("loading function: {symbol}");

                let lib = Library::new(path.clone())?;
                let func: libloading::Symbol<unsafe extern "C" fn() -> Language> =
                    lib.get(symbol.as_bytes())?;
                let language = func();
                mem::forget(lib);
                Some(language)
            }
        } else {
            loader = Loader::new()?;
            node_types.replace(path.join("src").join("node-types.json"));
            let language = loader
                .languages_at_path(path.as_path())?
                .pop()
                .ok_or(anyhow!("Can't find languages at path"))?;
            Some(language)
        }
    } else {
        None
    };

    language.ok_or(anyhow!("No language was fould"))
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

fn verify_times(source_dir: &Path, level: usize) -> Result<()> {
    let cache_dir = dirs::cache_dir()
        .ok_or(anyhow!("Cannot determine cache dir location"))?
        .join("tree-sitter")
        .join("lib");

    let library_path = |p| -> Result<_> {
        #[derive(Deserialize)]
        struct Grammar {
            name: String,
        }
        let grammar_json_data: Grammar = read_json_file(p)?;
        let mut library = cache_dir.join(grammar_json_data.name);
        library.set_extension(if cfg!(windows) { "dll" } else { "so" });
        Ok(library)
    };

    let mut files = Vec::new();
    let t = SystemTime::UNIX_EPOCH;
    files.push((0_usize, t, source_dir.clone().join("grammar.js")));
    files.push((0, t, source_dir.clone().join("src").join("grammar.json")));
    files.push((0, t, source_dir.clone().join("src").join("node-types.json")));
    files.push((1, t, source_dir.clone().join("src").join("parser.c")));
    files.push((2, t, library_path(&files.get(1).unwrap().2)?));

    for i in 0..files.len() {
        if i > level {
            break;
        }

        (&mut files[i]).1 = fs::metadata((&files[i]).2.as_path())?.modified()?;
        let (_, older, op) = &files[*&files[i].0];
        let (_, newer, np) = &files[i];

        if older > newer {
            return Err(anyhow!(
                "{} is older that {}: {:?} < {:?}",
                np.file_name().unwrap().to_str().unwrap(),
                op.file_name().unwrap().to_str().unwrap(),
                newer.duration_since(t).unwrap(),
                older.duration_since(t).unwrap(),
            ));
        }
    }

    Ok(())
}

mod info {
    use crate::{
        identifier,
        kind_merges::NameMerges,
        node_types::NodeTypes,
        parser_source::ParserSource,
        repr::{pad_len, BoolRepr},
    };
    use anyhow::{Ok, Result};
    use tree_sitter::Language;

    const B: &str = "\x1b[38;5;39m";
    const G: &str = "\x1b[38;5;155m";
    const Y: &str = "\x1b[38;5;227m";
    const P: &str = "\x1b[38;5;105m";
    const P2: &str = "\x1b[38;5;183m";
    const O: &str = "\x1b[38;5;208m";
    const R: &str = "\x1b[0m";

    pub(crate) fn print_language_info(
        language: &Language,
        node_types: Option<NodeTypes>,
        use_uppercase: bool,
        parser_source: Option<&ParserSource>,
    ) -> Result<()> {
        let kind_count = language.node_kind_count();
        let mut kind_merges = NameMerges::new();
        let sym_matcher = parser_source
            .map(|p| p.create_matcher(kind_count))
            .transpose()?;

        println!("fields (count: {}):", language.field_count());
        for id in 0..language.field_count() {
            if let Some(field) = language.field_name_for_id(id as u16) {
                let name = identifier::alt_name(field);
                let name_pad = pad_len(&name);
                println!("{id:5}: {field:?}{P2}{name:>name_pad$}{R}");
            }
        }

        println!();
        println!("kinds (count: {}):", kind_count);
        for id in 0..language.node_kind_count() {
            if let Some(kind) = language.node_kind_for_id(id as u16) {
                let named = language.node_kind_is_named(id as u16);
                let visible = language.node_kind_is_visible(id as u16);

                let (name, c) = if !named && visible {
                    let mut kind = identifier::sanitize(kind);
                    if use_uppercase {
                        kind = kind.to_uppercase();
                    }
                    Some((kind, P))
                } else if named && visible {
                    Some((identifier::alt_name(kind), P2))
                } else {
                    None
                }
                .unwrap_or_default();

                let mut leaf = false;
                if let Some(node_types) = node_types.as_ref() {
                    if let Some(node) = node_types.lookup(kind) {
                        leaf = node.is_leaf() && visible;
                    }
                };

                let name_pad = pad_len(&name);
                let (leaf, leaf_pad) = leaf.to_repr("leaf");
                let (named, named_pad) = named.to_repr("named");
                let (visible, visible_pad) = visible.to_repr("visible");

                let sym = sym_matcher
                    .as_ref()
                    .map(|m| m.get(id))
                    .flatten()
                    .unwrap_or_default();
                let sym_pad = pad_len(sym);

                println!("{id:5}: {kind:?}{c}{name:>name_pad$}{Y}{named:>named_pad$}{R}{G}{visible:>visible_pad$}{O}{leaf:>leaf_pad$}{B}{sym:>sym_pad$}{R}");

                if name.len() > 0 {
                    kind_merges.insert(id, &kind, &name);
                }
            }
        }

        kind_merges.finish();
        if let Some(merges) = kind_merges.merges() {
            println!();
            println!("kind merges: (count: {})", kind_merges.len());
            for (kind, merges) in merges {
                println!("  \"{kind}\"");
                for (id, kind, name) in merges {
                    let sym = if let Some(source) = parser_source {
                        source
                            .search_kind_id(*id)
                            .unwrap_or_default()
                            .unwrap_or_default()
                    } else {
                        ""
                    };
                    let sym_pad = pad_len(sym);
                    let name_pad = pad_len(&name);
                    println!("    {id:3}: {kind:?}{P2}{name:>name_pad$}{B}{sym:>sym_pad$}{R}");
                }
            }
        };

        Ok(())
    }
}

mod repr {
    pub(crate) trait BoolRepr {
        fn to_repr(&self, name: &'static str) -> (&'static str, usize);
    }

    impl BoolRepr for bool {
        fn to_repr(&self, name: &'static str) -> (&'static str, usize) {
            let repr = self.then_some(name).unwrap_or_default();
            let pad = pad_len(repr);
            (repr, pad)
        }
    }

    pub(crate) fn pad_len(s: &str) -> usize {
        if s.len() > 0 {
            s.len() + 1
        } else {
            0
        }
    }
}

mod identifier {
    pub(crate) fn alt_name(name: &str) -> String {
        use convert_case::{Case, Casing};
        name.from_case(Case::Camel).to_case(Case::UpperSnake)
    }

    // Based on sanitize_identifier() from: tree-sitter/cli/src/generate/render.rs
    pub(crate) fn sanitize(name: &str) -> String {
        fn replacement(c: char) -> Option<&'static str> {
            let replacement = match c {
                '~' => "TILDE",
                '`' => "BQUOTE",
                '!' => "BANG",
                '@' => "AT",
                '#' => "POUND",
                '$' => "DOLLAR",
                '%' => "PERCENT",
                '^' => "CARET",
                '&' => "AMP",
                '*' => "STAR",
                '(' => "LPAREN",
                ')' => "RPAREN",
                '-' => "DASH",
                '+' => "PLUS",
                '=' => "EQ",
                '{' => "LBRACE",
                '}' => "RBRACE",
                '[' => "LBRACK",
                ']' => "RBRACK",
                '\\' => "BSLASH",
                '|' => "PIPE",
                ':' => "COLON",
                ';' => "SEMI",
                '"' => "DQUOTE",
                '\'' => "SQUOTE",
                '<' => "LT",
                '>' => "GT",
                ',' => "COMMA",
                '.' => "DOT",
                '?' => "QMARK",
                '/' => "SLASH",
                '\n' => "LF",
                '\r' => "CR",
                '\t' => "TAB",

                // Numbers
                '0' => "ZERO",
                '1' => "ONE",
                '2' => "TWO",
                '3' => "THREE",
                '4' => "FOUR",
                '5' => "FIVE",
                '6' => "SIX",
                '7' => "SEVEN",
                '8' => "EIGHT",
                '9' => "NINE",

                '_' => "UNDERSCORE",

                _ => return None,
            };
            Some(replacement)
        }

        let mut replaced = false;
        let mut result = String::with_capacity(name.len());
        for c in name.chars() {
            if replaced {
                result.push('_');
            }
            if ('a' <= c && c <= 'z')
                || ('A' <= c && c <= 'Z')
                || ('0' <= c && c <= '9')
                || c == '_'
            {
                'a: {
                    'b: {
                        if result.len() == 0 {
                            let Some(replacement) = replacement(c) else {
                            break 'b
                        };
                            result += replacement;
                            replaced = true;
                            break 'a;
                        }
                    }
                    result.push(c);
                    replaced = false;
                }
            } else {
                let Some(replacement) = replacement(c) else {
                continue;
            };
                if !replaced && result.len() > 0 {
                    result.push('_');
                }
                result += replacement;
                replaced = true;
            }
        }
        result
    }

    pub(crate) fn replace_dashes_with_underscores(name: &str) -> String {
        let mut result = String::with_capacity(name.len());
        for c in name.chars() {
            if c == '-' {
                result.push('_');
            } else {
                result.push(c);
            }
        }
        result
    }
}

mod node_types {
    use crate::read_json_file;
    use anyhow::Result;
    use serde::{Deserialize, Serialize};
    use std::{collections::HashMap, path::PathBuf};

    #[derive(Serialize, Deserialize)]
    pub(crate) struct NodeTypes(Vec<Node>);

    #[derive(Serialize, Deserialize)]
    pub(crate) struct Node {
        r#type: String,
        named: bool,
        children: Option<NodeChildren>,
        fields: Option<HashMap<String, NodeChildren>>,
    }

    #[derive(Serialize, Deserialize)]
    pub(crate) struct NodeChildren {
        multiple: bool,
        required: bool,
        types: Vec<NodeRef>,
    }

    #[derive(Serialize, Deserialize)]
    pub(crate) struct NodeRef {
        r#type: String,
        named: bool,
    }

    impl NodeTypes {
        pub(crate) fn from_file(path: PathBuf) -> Result<Self> {
            read_json_file(path.as_path())
        }

        pub(crate) fn lookup(&self, name: &str) -> Option<&Node> {
            for node in self.0.iter() {
                if node.r#type == name {
                    return Some(node);
                }
            }
            None
        }
    }

    impl Node {
        pub(crate) fn is_leaf(&self) -> bool {
            self.children.is_none() && self.fields.is_none()
        }
    }
}

mod parser_source {
    use anyhow::Result;
    use regex::Regex;
    use std::{collections::HashMap, fs, path::PathBuf};

    pub(crate) struct ParserSource(String);

    pub(crate) struct SymMatcher(HashMap<usize, String>);

    impl ParserSource {
        pub(crate) fn from_file(path: PathBuf) -> Result<Self> {
            Ok(ParserSource(fs::read_to_string(path)?))
        }

        fn sym_re(id: &str) -> String {
            format!(r"\b([a-zA-Z_]*sym_[a-zA-Z0-9_]+)\b += +({id}),")
        }
        pub(crate) fn search_kind_id(&self, id: usize) -> Result<Option<&str>> {
            let re = Self::sym_re(id.to_string().as_str());
            let re = Regex::new(re.as_str())?;
            if let Some(captures) = re.captures(self.0.as_str()) {
                if let Some(m) = captures.get(1) {
                    return Ok(Some(m.as_str()));
                }
            }
            Ok(None)
        }

        pub(crate) fn create_matcher(&self, mut kind_cap: usize) -> Result<SymMatcher> {
            kind_cap -= 1; // exclude the `end` symbol
            let re = Regex::new(Self::sym_re(r"\d+").as_str())?;
            let mut syms = HashMap::<usize, String>::new();
            for captures in re.captures_iter(&self.0) {
                let mut captures = captures.iter();
                captures.next();
                let name = captures.next().flatten().unwrap().as_str();
                let id = captures.next().flatten().unwrap().as_str();
                // eprintln!("on: {id} --> {name}");
                if let Ok(uid) = id.parse::<usize>() {
                    syms.insert(uid, name.to_owned());
                } else {
                    // eprintln!("wrong id: {id}");
                };
                // This is optimization to early interrupt regex matching
                // when there is all symbols already matched.
                kind_cap -= 1;
                if kind_cap == 1 {
                    break;
                }
            }
            Ok(SymMatcher(syms))
        }
    }

    impl SymMatcher {
        pub(crate) fn get(&self, id: usize) -> Option<&str> {
            self.0.get(&id).map(|s| &**s)
        }
    }
}

mod kind_merges {
    use std::collections::{HashMap, HashSet};

    pub(crate) struct NameMerges {
        set: HashSet<String>,
        origins: HashMap<String, (usize, String, String)>,
        merges: HashMap<String, Vec<(usize, String, String)>>,
    }

    impl NameMerges {
        pub(crate) fn new() -> Self {
            Self {
                set: HashSet::new(),
                origins: HashMap::with_capacity(1),
                merges: HashMap::new(),
            }
        }

        pub(crate) fn insert(&mut self, id: usize, kind: &str, name: &str) {
            assert!(self.origins.capacity() > 0, "Clashes detector was finished");
            if !self.set.insert(kind.to_string()) {
                self.merges
                    .entry(kind.to_string())
                    .and_modify(|e| e.push((id, kind.to_string(), name.to_string())))
                    .or_insert_with(|| {
                        vec![
                            self.origins.remove(kind).unwrap(),
                            (id, kind.to_string(), name.to_string()),
                        ]
                    });
            } else {
                self.origins
                    .insert(kind.to_string(), (id, kind.to_string(), name.to_string()));
            }
        }

        pub(crate) fn merges(
            &self,
        ) -> Option<impl Iterator<Item = (&str, &[(usize, String, String)])>> {
            if self.merges.len() > 0 {
                Some(self.merges.iter().map(|s| (&**s.0, &**s.1)))
            } else {
                None
            }
        }

        pub(crate) fn len(&self) -> usize {
            self.merges.len()
        }

        pub(crate) fn finish(&mut self) {
            self.origins.clear();
            self.origins.shrink_to_fit();
            assert_eq!(self.origins.capacity(), 0);
        }
    }
}
