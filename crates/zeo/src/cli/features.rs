//! ruby's `--enable=<feature>` / `--disable=<feature>` family: the dials that
//! decide what a program starts with, before its own first line.
//!
//! The names are ruby's own, so a command line moves between the two engines
//! unchanged, and both of ruby's spellings work: `--disable=gems` and
//! `--disable-gems` say the same thing, as do `-` and `_` inside a name.
//!
//! Each dial's DEFAULT is a cargo feature of this crate (`ambient-gems` and
//! friends). A build therefore decides what its binaries start with, and the
//! command line still overrides that decision in either direction.

/// What one dial does when it is on.
#[derive(Clone, Copy)]
enum Kind {
    /// Require this library before the program's first line.
    Library(&'static str),
    /// Change zeo's own behaviour rather than the program's.
    Switch,
    /// ruby has the dial and zeo has nothing to put behind it. Turning it OFF
    /// names the state already in force, so it is accepted; turning it ON
    /// reports this reason rather than quietly doing nothing.
    Absent(&'static str),
}

struct Row {
    /// The canonical spelling, which is ruby's.
    name: &'static str,
    kind: Kind,
    default: bool,
}

/// Every dial, in the order `--help` and `ambient_libraries` report them.
const ROWS: &[Row] = &[
    Row {
        name: "gems",
        kind: Kind::Library("rubygems"),
        default: cfg!(feature = "ambient-gems"),
    },
    Row {
        name: "did_you_mean",
        kind: Kind::Library("did_you_mean"),
        default: cfg!(feature = "ambient-did-you-mean"),
    },
    Row {
        name: "error_highlight",
        kind: Kind::Library("error_highlight"),
        default: cfg!(feature = "ambient-error-highlight"),
    },
    Row {
        name: "syntax_suggest",
        kind: Kind::Absent("zeo does not bundle syntax_suggest"),
        default: false,
    },
    Row {
        name: "rubyopt",
        kind: Kind::Switch,
        default: true,
    },
    Row {
        name: "frozen-string-literal",
        kind: Kind::Absent(
            "zeo reads each file's own `# frozen_string_literal:` comment; there is no \
             whole-program dial",
        ),
        default: false,
    },
];

/// The dials one compile runs under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RubyFeatures {
    /// One flag per [`ROWS`] entry, in that order.
    on: [bool; ROWS.len()],
}

impl Default for RubyFeatures {
    fn default() -> RubyFeatures {
        let mut on = [false; ROWS.len()];
        for (slot, row) in on.iter_mut().zip(ROWS) {
            *slot = row.default;
        }
        RubyFeatures { on }
    }
}

/// One name as it is compared: ruby writes `did_you_mean` with underscores and
/// `frozen-string-literal` with hyphens, and nobody remembers which is which.
fn normalize(name: &str) -> String {
    name.trim().replace('-', "_")
}

impl RubyFeatures {
    /// One `--enable=<list>` or `--disable=<list>`. ruby takes a
    /// comma-separated list, and the name `all`.
    pub fn set(&mut self, list: &str, on: bool) -> Result<(), String> {
        for name in list.split(',').filter(|n| !n.trim().is_empty()) {
            self.set_one(name, on)?;
        }
        Ok(())
    }

    fn set_one(&mut self, name: &str, on: bool) -> Result<(), String> {
        let wanted = normalize(name);
        let verb = match on {
            true => "enable",
            false => "disable",
        };
        // `all` names the dials zeo can actually serve. An absent one stays
        // where it is rather than reporting its reason here, so `--enable=all`
        // means "everything you have" instead of failing on what nobody asked
        // for by name.
        if wanted == "all" {
            for (slot, row) in self.on.iter_mut().zip(ROWS) {
                if !matches!(row.kind, Kind::Absent(_)) {
                    *slot = on;
                }
            }
            return Ok(());
        }
        for (slot, row) in self.on.iter_mut().zip(ROWS) {
            if normalize(row.name) != wanted {
                continue;
            }
            if let (Kind::Absent(why), true) = (row.kind, on) {
                return Err(format!(
                    "--{verb}={} has nothing to turn on: {why}",
                    row.name
                ));
            }
            *slot = on;
            return Ok(());
        }
        Err(format!(
            "--{verb}={name} is not a feature zeo knows ({}, all)",
            ROWS.iter().map(|r| r.name).collect::<Vec<_>>().join(", ")
        ))
    }

    /// Whether a named dial is on. The name is ruby's, in either spelling.
    pub fn is_on(&self, name: &str) -> bool {
        let wanted = normalize(name);
        self.on
            .iter()
            .zip(ROWS)
            .any(|(on, row)| *on && normalize(row.name) == wanted)
    }

    /// The libraries to require before the program's first line, in the order
    /// ruby loads them: RubyGems first, because the other two are gems it
    /// activates.
    pub fn ambient_libraries(&self) -> Vec<String> {
        self.on
            .iter()
            .zip(ROWS)
            .filter(|(on, _)| **on)
            .filter_map(|(_, row)| match row.kind {
                Kind::Library(lib) => Some(lib.to_string()),
                Kind::Switch | Kind::Absent(_) => None,
            })
            .collect()
    }
}

/// The `--enable`/`--disable` help rows, rendered from the table so the two
/// can never disagree.
pub fn help_lines() -> Vec<String> {
    ROWS.iter()
        .map(|row| {
            let state = match row.default {
                true => "on",
                false => "off",
            };
            let what = match row.kind {
                Kind::Library(lib) => format!("require {lib} before the program"),
                Kind::Switch => "read RUBYOPT".to_string(),
                Kind::Absent(_) => "accepted; zeo has nothing behind it".to_string(),
            };
            format!("{:<24}{what} (default {state})", row.name)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dial_takes_either_spelling() {
        let mut f = RubyFeatures::default();
        f.set("gems", true).expect("gems is a dial");
        assert!(f.is_on("gems"));
        f.set("frozen_string_literal", false)
            .expect("the underscore spelling reaches the hyphenated row");
        f.set("frozen-string-literal", false)
            .expect("and so does the hyphenated one");
    }

    #[test]
    fn a_list_sets_every_name_in_it() {
        let mut f = RubyFeatures::default();
        f.set("gems,did_you_mean", true).expect("both are dials");
        assert_eq!(
            f.ambient_libraries(),
            vec!["rubygems".to_string(), "did_you_mean".to_string()]
        );
    }

    #[test]
    fn all_leaves_the_dials_zeo_cannot_serve() {
        let mut f = RubyFeatures::default();
        f.set("all", true).expect("all is always accepted");
        assert!(f.is_on("gems"));
        assert!(!f.is_on("syntax_suggest"));
    }

    #[test]
    fn enabling_an_absent_dial_reports_why() {
        let mut f = RubyFeatures::default();
        let err = f
            .set("syntax_suggest", true)
            .expect_err("zeo bundles no syntax_suggest");
        assert!(err.contains("does not bundle"), "{err}");
        // Turning it off names the state already in force.
        f.set("syntax_suggest", false).expect("off is where it is");
    }

    #[test]
    fn an_unknown_name_lists_the_ones_that_exist() {
        let err = RubyFeatures::default()
            .set("nope", false)
            .expect_err("`nope` is not a dial");
        assert!(err.contains("gems"), "{err}");
    }

    #[test]
    fn rubyopt_is_on_by_default_and_can_be_turned_off() {
        let mut f = RubyFeatures::default();
        assert!(f.is_on("rubyopt"));
        f.set("rubyopt", false).expect("rubyopt is a dial");
        assert!(!f.is_on("rubyopt"));
        // A switch is not a library.
        assert!(f.ambient_libraries().is_empty());
    }
}
