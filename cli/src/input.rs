use crate::parse::unescape_lf;
use anyhow::{anyhow, Context, Result};
use glob::glob;
use std::io::Read;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fs, io};
use tree_sitter::Language;
use tree_sitter_loader::Loader;

pub enum Input {
    File(PathBuf),
    Snippet(String),
}

pub struct Inputs(Vec<Input>);

impl Inputs {
    pub fn collect<'a>(
        paths_file: Option<&str>,
        paths: Option<impl Iterator<Item = &'a str>>,
    ) -> Result<Self> {
        let mut inputs = Vec::new();

        fn collect(path: &str, inputs: &mut Vec<Input>) -> Result<()> {
            let mut incorporate_path = |path: &str, positive| -> Result<()> {
                if positive {
                    inputs.push(Input::File(PathBuf::from_str(path)?));
                } else {
                    if let Some(index) = inputs.iter().position(|p| {
                        if let Input::File(p) = p {
                            p.as_os_str() == path
                        } else {
                            false
                        }
                    }) {
                        inputs.remove(index);
                    }
                }
                Ok(())
            };

            let mut path: &str = path;

            let mut positive = true;
            if path.starts_with("!") {
                positive = false;
                path = path.trim_start_matches("!");
            }

            if Path::new(path).exists() {
                incorporate_path(path, positive)?;
            } else {
                let paths =
                    glob(path).with_context(|| format!("Invalid glob pattern {:?}", path))?;
                for path in paths {
                    if let Some(path) = path?.to_str() {
                        incorporate_path(path, positive)?;
                    }
                }
            }
            Ok(())
        }

        if let Some(paths_file) = paths_file {
            let string = fs::read_to_string(paths_file)
                .with_context(|| format!("Failed to read paths file {}", paths_file))?;
            let paths = string.lines().map(|s| s.trim()).filter(|s| !s.is_empty());

            for path in paths {
                collect(path, &mut inputs)?;
            }
        };

        if let Some(paths) = paths {
            let mut next_snippet = false;
            for path in paths {
                if next_snippet {
                    inputs.push(Input::Snippet(path.to_string()));
                    next_snippet = false;
                    continue;
                }
                if path == "-" {
                    next_snippet = true;
                    continue;
                }

                collect(path, &mut inputs)?;
            }

            if next_snippet {
                let mut snippet = String::new();
                io::stdin().read_to_string(&mut snippet)?;
                inputs.push(Input::Snippet(snippet));
            }
        }

        if inputs.is_empty() {
            Err(anyhow!("Must provide one or more paths, globs or snippets"))
        } else {
            Ok(Self(inputs))
        }
    }
}

impl Deref for Inputs {
    type Target = [Input];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IntoIterator for Inputs {
    type Item = Input;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Inputs {
    pub fn into_parser_inputs<'a>(
        self,
        loader: &'a mut Loader,
        scope: Option<&'a str>,
        language_source_dir: Option<&'a Path>,
    ) -> ParserInputs<'a> {
        ParserInputs {
            loader,
            scope,
            language_source_dir,
            inputs: self.into_iter(),
            snippet_nr: 0,
        }
    }
}

pub struct ParserInput {
    pub source_code: Vec<u8>,
    pub language: Language,
    pub origin: String,
}

pub struct ParserInputs<'a> {
    loader: &'a mut Loader,
    scope: Option<&'a str>,
    language_source_dir: Option<&'a Path>,
    inputs: std::vec::IntoIter<Input>,
    snippet_nr: usize,
}

impl Iterator for ParserInputs<'_> {
    type Item = Result<ParserInput>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inputs.next().map(|input| -> Self::Item {
            let parser_input = match input {
                Input::File(path) => {
                    let path = Path::new(&path);
                    let source_code = fs::read(path)
                        .with_context(|| format!("Error reading source file {:?}", path))?;
                    let origin = path.to_string_lossy().to_string();
                    let language = self
                        .loader
                        .select_language(self.language_source_dir, self.scope, Some(path))
                        .with_context(|| format!("Can't find language for path `{origin}`"))?;
                    Ok(ParserInput {
                        source_code,
                        language,
                        origin,
                    })
                }
                Input::Snippet(snippet) => {
                    self.snippet_nr += 1;
                    let source_code = {
                        if cfg!(unix) {
                            snippet.into_bytes()
                        } else {
                            unescape_lf(&*snippet.into_bytes())
                        }
                    };
                    let origin = format!("Snippet #{}", self.snippet_nr);
                    let language = self
                        .loader
                        .select_language(self.language_source_dir, self.scope, None)
                        .with_context(|| format!("Can't find language for `{origin}`"))?;
                    Ok(ParserInput {
                        source_code,
                        language,
                        origin,
                    })
                }
            };
            parser_input
        })
    }
}

impl Inputs {
    pub fn max_path_length(&self) -> usize {
        self.iter()
            .map(|p| match p {
                Input::File(p) => p.to_str().map_or(0, |p| p.chars().count()),
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }
}

pub fn collect_paths<'a>(
    paths_file: Option<&str>,
    paths: Option<impl Iterator<Item = &'a str>>,
) -> Result<Vec<PathBuf>> {
    let inputs = Inputs::collect(paths_file, paths)?;
    let mut paths = Vec::with_capacity(inputs.0.len());
    for input in inputs.into_iter() {
        match input {
            Input::File(path) => paths.push(path),
            _ => return Err(anyhow!("This command doesn't support snippets yet")),
        }
    }
    Ok(paths)
}
