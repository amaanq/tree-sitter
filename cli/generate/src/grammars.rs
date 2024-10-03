use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use super::{
    nfa::Nfa,
    rules::{Alias, Associativity, Precedence, Rule, Symbol},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariableType {
    Hidden,
    Auxiliary,
    Anonymous,
    Named,
}

// Input grammar

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub kind: VariableType,
    pub rule: Rule,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrecedenceEntry {
    Name(String),
    Symbol(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct InputGrammar {
    pub name: String,
    pub variables: Vec<Variable>,
    pub extra_symbols: Vec<Rule>,
    pub expected_conflicts: Vec<Vec<String>>,
    pub precedence_orderings: Vec<Vec<PrecedenceEntry>>,
    pub external_tokens: Vec<Rule>,
    pub variables_to_inline: Vec<String>,
    pub supertype_symbols: Vec<String>,
    pub word_token: Option<String>,
}

impl InputGrammar {
    fn rule_is_used(&self, rule: &Rule) -> bool {
        self.variables.iter().any(|v| match rule {
            Rule::NamedSymbol(name) => v.name == *name,
            Rule::Choice(rules) => rules.iter().any(|r| self.rule_is_used(r)),
            Rule::Metadata { rule, .. } => self.rule_is_used(rule),
            Rule::Repeat(inner) => self.rule_is_used(inner),
            Rule::Seq(rules) => rules.iter().any(|r| self.rule_is_used(r)),
            Rule::Blank | Rule::String(_) | Rule::Pattern(_, _) | Rule::Symbol(_) => false,
        })
    }

    pub fn remove_unused_rules(&mut self) {
        let mut to_remove = HashMap::new();

        for (i, variable) in self.variables.iter().enumerate() {
            if !self.rule_is_used(&variable.rule) {
                to_remove.insert(variable.name.clone(), i);
            }
        }

        // remove variables
        self.variables
            .retain(|v| !to_remove.contains_key(v.name.as_str()));

        // remove extras
        self.extra_symbols.retain(|r| match r {
            Rule::NamedSymbol(name) | Rule::String(name) => !to_remove.contains_key(name.as_str()),
            _ => true,
        });

        // remove external tokens
        self.external_tokens.retain(|r| match r {
            Rule::NamedSymbol(name) | Rule::String(name) => !to_remove.contains_key(name.as_str()),
            _ => true,
        });

        // remove variables to inline
        self.variables_to_inline
            .retain(|name| !to_remove.contains_key(name.as_str()));

        // remove supertype symbols
        self.supertype_symbols
            .retain(|name| !to_remove.contains_key(name.as_str()));
    }
}

// Extracted lexical grammar

#[derive(Debug, PartialEq, Eq)]
pub struct LexicalVariable {
    pub name: String,
    pub kind: VariableType,
    pub implicit_precedence: i32,
    pub start_state: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LexicalGrammar {
    pub nfa: Nfa,
    pub variables: Vec<LexicalVariable>,
}

// Extracted syntax grammar

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProductionStep {
    pub symbol: Symbol,
    pub precedence: Precedence,
    pub associativity: Option<Associativity>,
    pub alias: Option<Alias>,
    pub field_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Production {
    pub steps: Vec<ProductionStep>,
    pub dynamic_precedence: i32,
}

#[derive(Default)]
pub struct InlinedProductionMap {
    pub productions: Vec<Production>,
    pub production_map: HashMap<(*const Production, u32), Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxVariable {
    pub name: String,
    pub kind: VariableType,
    pub productions: Vec<Production>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalToken {
    pub name: String,
    pub kind: VariableType,
    pub corresponding_internal_token: Option<Symbol>,
}

#[derive(Debug, Default)]
pub struct SyntaxGrammar {
    pub variables: Vec<SyntaxVariable>,
    pub extra_symbols: Vec<Symbol>,
    pub expected_conflicts: Vec<Vec<Symbol>>,
    pub external_tokens: Vec<ExternalToken>,
    pub supertype_symbols: Vec<Symbol>,
    pub variables_to_inline: Vec<Symbol>,
    pub word_token: Option<Symbol>,
    pub precedence_orderings: Vec<Vec<PrecedenceEntry>>,
}

#[cfg(test)]
impl ProductionStep {
    #[must_use]
    pub const fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            precedence: Precedence::None,
            associativity: None,
            alias: None,
            field_name: None,
        }
    }

    pub fn with_prec(self, precedence: Precedence, associativity: Option<Associativity>) -> Self {
        Self {
            symbol: self.symbol,
            precedence,
            associativity,
            alias: self.alias,
            field_name: self.field_name,
        }
    }

    pub fn with_alias(self, value: &str, is_named: bool) -> Self {
        Self {
            symbol: self.symbol,
            precedence: self.precedence,
            associativity: self.associativity,
            alias: Some(Alias {
                value: value.to_string(),
                is_named,
            }),
            field_name: self.field_name,
        }
    }
    pub fn with_field_name(self, name: &str) -> Self {
        Self {
            symbol: self.symbol,
            precedence: self.precedence,
            associativity: self.associativity,
            alias: self.alias,
            field_name: Some(name.to_string()),
        }
    }
}

impl Production {
    pub fn first_symbol(&self) -> Option<Symbol> {
        self.steps.first().map(|s| s.symbol)
    }
}

#[cfg(test)]
impl Variable {
    pub fn named(name: &str, rule: Rule) -> Self {
        Self {
            name: name.to_string(),
            kind: VariableType::Named,
            rule,
        }
    }

    pub fn auxiliary(name: &str, rule: Rule) -> Self {
        Self {
            name: name.to_string(),
            kind: VariableType::Auxiliary,
            rule,
        }
    }

    pub fn hidden(name: &str, rule: Rule) -> Self {
        Self {
            name: name.to_string(),
            kind: VariableType::Hidden,
            rule,
        }
    }

    pub fn anonymous(name: &str, rule: Rule) -> Self {
        Self {
            name: name.to_string(),
            kind: VariableType::Anonymous,
            rule,
        }
    }
}

impl VariableType {
    pub fn is_visible(self) -> bool {
        self == Self::Named || self == Self::Anonymous
    }
}

impl LexicalGrammar {
    pub fn variable_indices_for_nfa_states<'a>(
        &'a self,
        state_ids: &'a [u32],
    ) -> impl Iterator<Item = usize> + 'a {
        let mut prev = None;
        state_ids.iter().filter_map(move |state_id| {
            let variable_id = self.variable_index_for_nfa_state(*state_id);
            if prev == Some(variable_id) {
                None
            } else {
                prev = Some(variable_id);
                prev
            }
        })
    }

    pub fn variable_index_for_nfa_state(&self, state_id: u32) -> usize {
        self.variables
            .iter()
            .position(|v| v.start_state >= state_id)
            .unwrap()
    }
}

impl SyntaxVariable {
    pub fn is_auxiliary(&self) -> bool {
        self.kind == VariableType::Auxiliary
    }

    pub fn is_hidden(&self) -> bool {
        self.kind == VariableType::Hidden || self.kind == VariableType::Auxiliary
    }
}

impl InlinedProductionMap {
    pub fn inlined_productions<'a>(
        &'a self,
        production: &Production,
        step_index: u32,
    ) -> Option<impl Iterator<Item = &'a Production> + 'a> {
        self.production_map
            .get(&(production as *const Production, step_index))
            .map(|production_indices| {
                production_indices
                    .iter()
                    .copied()
                    .map(move |index| &self.productions[index])
            })
    }
}

impl fmt::Display for PrecedenceEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Name(n) => write!(f, "'{n}'"),
            Self::Symbol(s) => write!(f, "$.{s}"),
        }
    }
}
