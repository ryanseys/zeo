//! `zeo dev <command>`: bindings the Ruby toolchain cannot get any other way.
//!
//! Hidden from `--help` on purpose. These are not part of zeo's CLI contract
//! -- they exist because `tools/` runs on ruby and two facts about the tree
//! live behind a `syn` parse of the runtime's own source.
//!
//! Everything else the tools need they read for themselves. A binding here has
//! to earn its place by being unreachable from ruby.

use std::path::PathBuf;

/// Dispatch a `dev` subcommand. `Ok(())` means the command ran.
pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("scan-decls") => scan_decls(args.get(1).map(PathBuf::from)),
        Some(other) => Err(format!("unknown dev command `{other}` (try: scan-decls)")),
        None => Err("dev takes a command (try: scan-decls)".to_string()),
    }
}

/// The `ruby_class!`/`ruby_module!` declarations and the `zeo-abi` `BUILTINS`
/// rows, as JSON on stdout.
///
/// The join key both halves share is the `ClassId` const ident -- a header
/// spells the class `Stat` where the Ruby name is `File::Stat`, and only
/// `BUILTINS` knows that. Emitting both halves lets the caller do the join,
/// so no policy of any one tool is baked in here.
fn scan_decls(root: Option<PathBuf>) -> Result<(), String> {
    let root = match root {
        Some(p) => p,
        None => {
            std::env::current_dir().map_err(|e| format!("reading the working directory: {e}"))?
        }
    };
    if !root.join("crates/zeo-rt/src").is_dir() {
        return Err(format!(
            "{} is not a zeo checkout (no crates/zeo-rt/src)",
            root.display()
        ));
    }
    print!("{}", scan_json(&root));
    Ok(())
}

fn scan_json(root: &std::path::Path) -> String {
    let mut out = String::from("{\"abi\":{");
    let abi = zeo_dsl::scan::scan_abi(root);
    for (i, (class_const, c)) in abi.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(&mut out, class_const);
        out.push_str(":{\"ruby_name\":");
        json_str(&mut out, &c.ruby_name);
        out.push_str(",\"is_module\":");
        out.push_str(if c.is_module { "true" } else { "false" });
        out.push_str(",\"feature\":");
        json_opt(&mut out, c.feature.as_deref());
        out.push_str(",\"superclass\":");
        json_opt(&mut out, c.superclass.as_deref());
        out.push_str(",\"includes\":[");
        for (j, inc) in c.includes.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            json_str(&mut out, inc);
        }
        out.push_str("]}");
    }
    out.push_str("},\"decls\":[");
    for (i, d) in zeo_dsl::scan::scan_decls(root).iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"class_const\":");
        json_str(&mut out, &d.class_const);
        out.push_str(",\"kind\":");
        json_str(&mut out, d.kind.tag());
        out.push_str(",\"name\":");
        json_str(&mut out, &d.name);
        out.push_str(&format!(
            ",\"arity\":{},\"derived\":{},\"override_written\":{},",
            d.arity, d.derived, d.override_written
        ));
        out.push_str("\"file\":");
        json_str(&mut out, &d.file);
        out.push_str(&format!(",\"line\":{},", d.line));
        out.push_str("\"cfg\":");
        json_opt(&mut out, d.cfg.as_deref());
        out.push_str(",\"is_private\":");
        out.push_str(if d.is_private { "true" } else { "false" });
        out.push('}');
    }
    out.push_str("]}\n");
    out
}

fn json_opt(out: &mut String, v: Option<&str>) {
    match v {
        Some(s) => json_str(out, s),
        None => out.push_str("null"),
    }
}

/// A JSON string. The scanner's output is Rust identifiers, Ruby method names
/// and repo-relative paths, so only the mandatory escapes can ever appear --
/// but a `cfg` renders arbitrary source, so this escapes properly.
fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Whether `argv` opens with the hidden `dev` command.
pub fn is_dev(argv: &[String]) -> bool {
    argv.first().is_some_and(|a| a == "dev")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bare_word_opens_the_hidden_command() {
        assert!(is_dev(&["dev".to_string()]));
        assert!(!is_dev(&["dev.rb".to_string()]));
        assert!(!is_dev(&[]));
    }

    #[test]
    fn a_cfg_string_survives_its_quotes() {
        // A rendered `#[cfg]` is the one scanned field that carries quotes,
        // so it is the field a naive writer breaks.
        let mut out = String::new();
        json_str(&mut out, r#"cfg(target_vendor = "apple")"#);
        assert_eq!(out, r#""cfg(target_vendor = \"apple\")""#);
    }

    #[test]
    fn a_control_byte_escapes_rather_than_writing_itself() {
        let mut out = String::new();
        json_str(&mut out, "a\u{1}b\n");
        assert_eq!(out, r#""a\u0001b\n""#);
    }

    #[test]
    fn an_absent_field_is_null_not_an_empty_string() {
        let mut out = String::new();
        json_opt(&mut out, None);
        json_opt(&mut out, Some(""));
        assert_eq!(out, "null\"\"");
    }

    #[test]
    fn a_directory_that_is_not_a_checkout_is_refused() {
        let err = scan_decls(Some(PathBuf::from("/"))).unwrap_err();
        assert!(err.contains("not a zeo checkout"), "{err}");
    }

    #[test]
    fn an_unknown_dev_command_names_the_one_that_exists() {
        let err = run(&["census".to_string()]).unwrap_err();
        assert!(err.contains("scan-decls"), "{err}");
    }
}
