//! Reading the Makefile mkmf wrote.
//!
//! This is not a `make` implementation and does not try to be. mkmf's output
//! has a fixed shape -- a block of `NAME = value` assignments, then a fixed
//! set of rules -- and everything zeo needs to compile and link an extension
//! is in the assignments.
//!
//! # The rule that keeps this honest
//!
//! An `extconf.rb` may append its own rules after `create_makefile`, and a
//! gem with a `depend` file gets those folded in too. Driving the build
//! ourselves would silently skip every one of them.
//!
//! So [`Makefile::rules`] records every target that has a recipe, and
//! [`super::build`] refuses to drive a Makefile carrying a rule mkmf did not
//! write -- it runs real `make` instead. A custom rule is therefore slower
//! and never wrong, which is the right way round.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// mkmf's own rules. A target outside this set means the Makefile was
/// extended, and [`Makefile::is_stock`] answers false.
///
/// The suffix rules are matched by shape rather than listed, because mkmf
/// emits a pair per source extension and the set grows with the extension
/// list rather than with anything zeo does.
const STOCK_TARGETS: &[&str] = &[
    "all",
    "static",
    "clean",
    "clean-so",
    "clean-rb",
    "clean-rb-default",
    "clean-static",
    "distclean",
    "distclean-so",
    "distclean-rb",
    "distclean-rb-default",
    "distclean-static",
    "realclean",
    "install",
    "install-so",
    "install-rb",
    "install-rb-default",
    "pre-install-rb",
    "pre-install-rb-default",
    "do-install-rb",
    "do-install-rb-default",
    "site-install",
    "site-install-so",
    "site-install-rb",
    ".SUFFIXES",
    ".PHONY",
    "$(TARGET_SO)",
    "$(OBJS)",
    "$(TARGET_SO_DIR_TIMESTAMP)",
    "$(STATIC_LIB)",
    "$(RUBY_EXTCONF_H)",
    "Makefile",
];

pub struct Makefile {
    vars: BTreeMap<String, String>,
    /// Every target that carries a recipe, verbatim as written.
    rules: BTreeSet<String>,
}

impl Makefile {
    /// Parse the assignments and collect the rule targets.
    ///
    /// A recipe line starts with a TAB -- that is `make`'s own rule and the
    /// only thing separating a recipe from a rule header.
    pub fn parse(text: &str) -> Makefile {
        let mut vars = BTreeMap::new();
        let mut rules = BTreeSet::new();
        let mut in_recipe = false;
        for line in text.lines() {
            if line.starts_with('\t') {
                continue;
            }
            in_recipe = false;
            let trimmed = line.trim_end();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((name, value)) = assignment(trimmed) {
                vars.insert(name, value);
                continue;
            }
            // A rule header: `target: prerequisites`, possibly `::`.
            if let Some((targets, _)) = trimmed.split_once(':') {
                for t in targets.split_whitespace() {
                    rules.insert(t.trim_end_matches(':').to_string());
                }
                in_recipe = true;
            }
        }
        let _ = in_recipe;
        Makefile { vars, rules }
    }

    /// One variable, with every `$(NAME)` reference resolved.
    ///
    /// An unknown reference expands to nothing, which is what `make` does
    /// with an unset variable -- and is why `$(ARCH_FLAG)` in a `CFLAGS`
    /// contributes no empty argument.
    pub fn get(&self, name: &str) -> String {
        self.expand(name, &mut Vec::new())
    }

    /// The same, split on whitespace, with empties dropped.
    pub fn words(&self, name: &str) -> Vec<String> {
        self.get(name)
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    fn expand(&self, name: &str, seen: &mut Vec<String>) -> String {
        // A self-referential variable would loop forever. `make` reports one
        // as an error; answering the empty string keeps the build going and
        // the failure lands on the compiler, which names the flag.
        if seen.iter().any(|s| s == name) {
            return String::new();
        }
        let Some(raw) = self.vars.get(name) else {
            return String::new();
        };
        seen.push(name.to_string());
        let out = self.substitute(raw, seen);
        seen.pop();
        out
    }

    fn substitute(&self, text: &str, seen: &mut Vec<String>) -> String {
        let mut out = String::with_capacity(text.len());
        let bytes: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != '$' || i + 1 >= bytes.len() {
                out.push(bytes[i]);
                i += 1;
                continue;
            }
            let open = bytes[i + 1];
            let close = match open {
                '(' => ')',
                '{' => '}',
                // `$$` is a literal dollar, and a bare `$x` is one character.
                '$' => {
                    out.push('$');
                    i += 2;
                    continue;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                    continue;
                }
            };
            let Some(end) = bytes[i + 2..].iter().position(|c| *c == close) else {
                out.push(bytes[i]);
                i += 1;
                continue;
            };
            let name: String = bytes[i + 2..i + 2 + end].iter().collect();
            out.push_str(&self.expand(&name, seen));
            i += 2 + end + 1;
        }
        out
    }

    /// Did mkmf write every rule in this file?
    ///
    /// A suffix rule (`.c.o`) is recognised by shape: mkmf emits one pair per
    /// source extension, and the set grows with that list rather than with
    /// anything zeo decides.
    pub fn is_stock(&self) -> bool {
        self.rules.iter().all(|t| {
            STOCK_TARGETS.contains(&t.as_str()) || is_suffix_rule(t) || t.starts_with("$(")
        })
    }

    /// Every target this file defines a recipe for. Named so a refusal can
    /// say WHICH rule sent the build to `make`.
    pub fn extra_rules(&self) -> Vec<&str> {
        self.rules
            .iter()
            .filter(|t| {
                !STOCK_TARGETS.contains(&t.as_str()) && !is_suffix_rule(t) && !t.starts_with("$(")
            })
            .map(String::as_str)
            .collect()
    }
}

/// `.c.o`, `.cpp.S` and the rest: two dot-prefixed extensions and nothing
/// else.
fn is_suffix_rule(target: &str) -> bool {
    let rest = match target.strip_prefix('.') {
        Some(r) => r,
        None => return false,
    };
    let parts: Vec<&str> = rest.split('.').collect();
    parts.len() == 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(char::is_alphanumeric))
}

/// `NAME = value`, `NAME := value`, `NAME ?= value` or `NAME += value`.
///
/// mkmf writes only the plain form, but the others cost one comparison and a
/// misread assignment would become a rule target instead.
fn assignment(line: &str) -> Option<(String, String)> {
    let eq = line.find('=')?;
    if eq == 0 {
        return None;
    }
    let (mut name, mut value) = line.split_at(eq);
    value = &value[1..];
    // A rule header can carry an `=` after the colon; an assignment cannot
    // have a colon before the `=` unless it is `:=`.
    if let Some(colon) = name.find(':')
        && colon + 1 != name.len()
    {
        return None;
    }
    name = name.trim_end_matches([':', '?', '+']);
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some((name.to_string(), value.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
srcdir = .
CC = cc
optflags = -O2
debugflags = -g
cflags = $(optflags) $(debugflags)
CCDLFLAGS = -fPIC
ARCH_FLAG =
CFLAGS = $(CCDLFLAGS) $(cflags) $(ARCH_FLAG)
INCFLAGS = -I. -I$(srcdir)
OBJS = probe.o other.o
TARGET = probe
DLLIB = $(TARGET).bundle
TARGET_SO = $(DLLIB)

all: $(DLLIB)

$(TARGET_SO): $(OBJS) Makefile
\t$(LDSHARED) -o $@ $(OBJS)

.c.o:
\t$(CC) $(CFLAGS) -c $<
";

    #[test]
    fn a_reference_expands_through_every_level() {
        let mk = Makefile::parse(SAMPLE);
        assert_eq!(mk.get("CFLAGS"), "-fPIC -O2 -g ");
        assert_eq!(mk.words("CFLAGS"), ["-fPIC", "-O2", "-g"]);
        assert_eq!(mk.get("DLLIB"), "probe.bundle");
        assert_eq!(mk.get("TARGET_SO"), "probe.bundle");
        assert_eq!(mk.words("OBJS"), ["probe.o", "other.o"]);
    }

    /// An unset variable expands to nothing, as `make` does -- so an empty
    /// `$(ARCH_FLAG)` contributes no argument rather than an empty one.
    #[test]
    fn an_unset_reference_contributes_nothing() {
        let mk = Makefile::parse(SAMPLE);
        assert_eq!(mk.get("NOPE"), "");
        assert!(mk.words("CFLAGS").iter().all(|w| !w.is_empty()));
        // `LDSHARED` is referenced by a recipe and never assigned.
        assert_eq!(mk.get("LDSHARED"), "");
    }

    /// A variable naming itself would loop forever. `make` errors; answering
    /// empty keeps the build going and lets the compiler name the flag.
    #[test]
    fn a_self_reference_terminates() {
        let mk = Makefile::parse("A = $(B)\nB = $(A) x\n");
        assert_eq!(mk.get("A"), " x");
    }

    #[test]
    fn a_stock_makefile_is_recognised_and_an_extended_one_is_not() {
        let mk = Makefile::parse(SAMPLE);
        assert!(mk.is_stock(), "extra rules: {:?}", mk.extra_rules());

        let extended = format!("{SAMPLE}\nprobe.o: generated.h\n\t$(CC) -c probe.c\n");
        let mk = Makefile::parse(&extended);
        assert!(!mk.is_stock());
        assert_eq!(mk.extra_rules(), ["probe.o"]);
    }

    /// A recipe line is the one that starts with a TAB. Reading one as a
    /// rule header would invent targets out of every `$(CC) ...` line.
    #[test]
    fn a_recipe_line_is_not_a_rule_header() {
        let mk = Makefile::parse(SAMPLE);
        assert!(
            !mk.rules.iter().any(|t| t.contains('$') && t.contains("CC")),
            "a recipe was read as a rule: {:?}",
            mk.rules
        );
    }

    #[test]
    fn a_dollar_dollar_is_a_literal_dollar() {
        let mk = Makefile::parse("A = a$$b\n");
        assert_eq!(mk.get("A"), "a$b");
    }
}
