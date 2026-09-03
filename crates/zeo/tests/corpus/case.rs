//! One corpus program as a file: `#@` directives, the program, and below
//! `__END__` the answer `cargo xtask bless` recorded.
//!
//! Ruby stops lexing at `__END__`, so the trailer costs the program nothing
//! and can hold any bytes. The grammar is `docs/reference/test-format.md`;
//! this file is the one reader and the one writer of it, shared by the
//! harness and by `cargo xtask bless` through `#[path]`.

use std::path::Path;

/// A directive line: `#@ key` or `#@ key: value`, at column 0, above `__END__`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Directives {
    /// ARGV, for both engines.
    pub args: Vec<String>,
    /// A file piped to stdin, relative to the program's directory.
    pub stdin: Option<String>,
    /// Environment for both engines.
    pub env: Vec<(String, String)>,
    /// Flags for the oracle only.
    pub ruby: Vec<String>,
    /// Flags for the zeo CLI only, before the file.
    pub zeo: Vec<String>,
    /// Environment for the zeo child only.
    pub zeo_env: Vec<(String, String)>,
    /// The one platform the program runs on.
    pub only: Option<Platform>,
    /// `jit`: the program needs the compiler and itself in one process.
    pub backend: Option<Backend>,
    /// The exit cycle census the memcheck leg expects.
    pub gccheck: Option<String>,
    /// Gaps only: the divergence is between the spliced and packaged roads.
    pub pkggap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Macos,
    Linux,
}

impl Platform {
    pub fn host() -> Platform {
        if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Macos
        }
    }

    fn parse(word: &str) -> Result<Platform, String> {
        match word {
            "macos" => Ok(Platform::Macos),
            "linux" => Ok(Platform::Linux),
            other => Err(format!("unknown platform {other:?} (macos or linux)")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Jit,
}

/// How a program ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
}

impl Default for Exit {
    fn default() -> Self {
        Exit::Code(0)
    }
}

impl std::fmt::Display for Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Exit::Code(c) => write!(f, "exit {c}"),
            Exit::Signal(s) => write!(f, "signal {s}"),
        }
    }
}

/// What a program printed and how it ended.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Answer {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: Exit,
}

/// The recorded answer, and the linux one when the two differ.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Trailer {
    pub answer: Answer,
    pub linux: Option<Answer>,
}

impl Trailer {
    /// The answer this platform is held to.
    pub fn for_host(&self) -> &Answer {
        match (Platform::host(), &self.linux) {
            (Platform::Linux, Some(linux)) => linux,
            _ => &self.answer,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// Everything above the `__END__` line.
    pub program: Vec<u8>,
    pub directives: Directives,
    pub trailer: Option<Trailer>,
}

const END: &[u8] = b"__END__";

/// Where the `__END__` line starts, and where the trailer starts after it.
fn split(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut at = 0;
    while at <= bytes.len() {
        let rest = &bytes[at..];
        if rest.starts_with(END) {
            let after = &rest[END.len()..];
            if after.is_empty() {
                return Some((at, bytes.len()));
            }
            if after.starts_with(b"\n") {
                return Some((at, at + END.len() + 1));
            }
        }
        match rest.iter().position(|&b| b == b'\n') {
            Some(nl) => at += nl + 1,
            None => break,
        }
    }
    None
}

impl Case {
    pub fn parse(bytes: &[u8]) -> Result<Case, String> {
        let (program, trailer) = match split(bytes) {
            Some((end, after)) => (&bytes[..end], Some(&bytes[after..])),
            None => (bytes, None),
        };
        Ok(Case {
            program: program.to_vec(),
            directives: Directives::parse(program)?,
            trailer: trailer.map(Trailer::parse).transpose()?,
        })
    }

    pub fn read(path: &Path) -> Result<Case, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Case::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The file's bytes for `program` with `trailer` recorded under it.
    pub fn render(program: &[u8], trailer: &Trailer) -> Vec<u8> {
        let mut out = program.to_vec();
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(END);
        out.push(b'\n');
        out.extend_from_slice(&trailer.render());
        out
    }
}

impl Directives {
    /// Every `#@` line in `program`. An unknown key is an error: a directive
    /// nobody reads would silently test something else.
    pub fn parse(program: &[u8]) -> Result<Directives, String> {
        let mut d = Directives::default();
        for (n, line) in program.split(|&b| b == b'\n').enumerate() {
            let Some(rest) = line.strip_prefix(b"#@") else {
                continue;
            };
            let rest = String::from_utf8_lossy(rest);
            let rest = rest.trim();
            let (key, value) = match rest.split_once(':') {
                Some((k, v)) => (k.trim(), Some(v.trim())),
                None => (rest, None),
            };
            let value_of =
                || value.ok_or_else(|| format!("line {}: `#@ {key}` needs a value", n + 1));
            match key {
                "args" => d.args = words(value_of()?),
                "stdin" => d.stdin = Some(value_of()?.to_string()),
                "env" => d.env = pairs(value_of()?)?,
                "ruby" => d.ruby = words(value_of()?),
                "zeo" => d.zeo = words(value_of()?),
                "zeo-env" => d.zeo_env = pairs(value_of()?)?,
                "only" => d.only = Some(Platform::parse(value_of()?)?),
                "backend" => {
                    d.backend = match value_of()? {
                        "jit" => Some(Backend::Jit),
                        other => {
                            return Err(format!(
                                "line {}: `#@ backend: {other}` -- only `jit` is a directive; every program takes the AOT legs unless it says jit",
                                n + 1
                            ));
                        }
                    }
                }
                "gccheck" => d.gccheck = Some(value_of()?.to_string()),
                "pkggap" => d.pkggap = true,
                other => {
                    return Err(format!(
                        "line {}: unknown directive `#@ {other}` (args, stdin, env, ruby, zeo, zeo-env, only, backend, gccheck, pkggap)",
                        n + 1
                    ));
                }
            }
        }
        Ok(d)
    }
}

fn words(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_string).collect()
}

fn pairs(value: &str) -> Result<Vec<(String, String)>, String> {
    value
        .split_whitespace()
        .map(|w| {
            w.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| format!("{w:?} is not KEY=VALUE"))
        })
        .collect()
}

// ---- the trailer ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Stdout,
    Stderr,
}

enum Marker {
    Section(bool, Section),
    Exit(bool, Exit),
    Nonl,
}

/// A marker line, or `None` for a body line. A `#@` line that is not a
/// marker is an error: body lines that begin with `#@` are escaped, so an
/// unescaped one is a typo in the trailer.
fn marker(text: &[u8]) -> Result<Option<Marker>, String> {
    let Some(rest) = text.strip_prefix(b"#@") else {
        return Ok(None);
    };
    let rest = String::from_utf8_lossy(rest);
    let mut words = rest.split_whitespace();
    let mut word = words.next().unwrap_or("");
    let linux = word == "linux";
    if linux {
        word = words.next().unwrap_or("");
    }
    let number: Option<i32> = words.next().and_then(|n| n.parse().ok());
    let number =
        || number.ok_or_else(|| format!("trailer marker `{}` needs a number", rest.trim()));
    let m = match word {
        "stdout" => Marker::Section(linux, Section::Stdout),
        "stderr" => Marker::Section(linux, Section::Stderr),
        "exit" => Marker::Exit(linux, Exit::Code(number()?)),
        "signal" => Marker::Exit(linux, Exit::Signal(number()?)),
        "nonl" if !linux => Marker::Nonl,
        _ => return Err(format!("unknown trailer marker `#@{rest}`")),
    };
    Ok(Some(m))
}

/// A body line as written: one more `#` in front of anything that could
/// read as a marker.
fn escape(line: &[u8], out: &mut Vec<u8>) {
    let hashes = line.iter().take_while(|&&b| b == b'#').count();
    if hashes > 0 && line.get(hashes) == Some(&b'@') {
        out.push(b'#');
    }
    out.extend_from_slice(line);
}

/// The inverse of [`escape`].
fn unescape(line: &[u8]) -> &[u8] {
    let hashes = line.iter().take_while(|&&b| b == b'#').count();
    if hashes >= 2 && line.get(hashes) == Some(&b'@') {
        &line[1..]
    } else {
        line
    }
}

fn target(t: &mut Trailer, linux: bool) -> &mut Answer {
    if linux {
        t.linux.get_or_insert_with(Answer::default)
    } else {
        &mut t.answer
    }
}

impl Trailer {
    pub fn parse(bytes: &[u8]) -> Result<Trailer, String> {
        let mut t = Trailer::default();
        let mut linux = false;
        let mut section = Section::Stdout;
        for line in bytes.split_inclusive(|&b| b == b'\n') {
            let text = line.strip_suffix(b"\n").unwrap_or(line);
            match marker(text)? {
                Some(Marker::Section(l, s)) => {
                    linux = l;
                    section = s;
                    target(&mut t, linux);
                }
                Some(Marker::Exit(l, exit)) => target(&mut t, l).exit = exit,
                Some(Marker::Nonl) => {
                    let answer = target(&mut t, linux);
                    let body = match section {
                        Section::Stdout => &mut answer.stdout,
                        Section::Stderr => &mut answer.stderr,
                    };
                    if body.pop() != Some(b'\n') {
                        return Err("`#@ nonl` must follow a line".into());
                    }
                }
                None => {
                    let answer = target(&mut t, linux);
                    let body = match section {
                        Section::Stdout => &mut answer.stdout,
                        Section::Stderr => &mut answer.stderr,
                    };
                    body.extend_from_slice(unescape(text));
                    if line.ends_with(b"\n") {
                        body.push(b'\n');
                    }
                }
            }
        }
        Ok(t)
    }

    pub fn render(&self) -> Vec<u8> {
        let mut out = Vec::new();
        render_answer(&self.answer, "", &mut out);
        if let Some(linux) = &self.linux {
            out.extend_from_slice(b"#@ linux stdout\n");
            render_answer(linux, "linux ", &mut out);
        }
        out
    }
}

fn render_answer(answer: &Answer, prefix: &str, out: &mut Vec<u8>) {
    render_body(&answer.stdout, out);
    if !answer.stderr.is_empty() {
        out.extend_from_slice(format!("#@ {prefix}stderr\n").as_bytes());
        render_body(&answer.stderr, out);
    }
    if answer.exit != Exit::Code(0) {
        out.extend_from_slice(format!("#@ {prefix}{}\n", answer.exit).as_bytes());
    }
}

fn render_body(body: &[u8], out: &mut Vec<u8>) {
    if body.is_empty() {
        return;
    }
    for line in body.split_inclusive(|&b| b == b'\n') {
        escape(line, out);
    }
    if !body.ends_with(b"\n") {
        out.extend_from_slice(b"\n#@ nonl\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(stdout: &str, stderr: &str, exit: Exit) -> Answer {
        Answer {
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit,
        }
    }

    fn round_trip(t: &Trailer) {
        let rendered = t.render();
        let back = Trailer::parse(&rendered)
            .unwrap_or_else(|e| panic!("{e}\n{}", String::from_utf8_lossy(&rendered)));
        assert_eq!(&back, t, "{}", String::from_utf8_lossy(&rendered));
    }

    #[test]
    fn a_plain_answer_is_the_bytes_under_end() {
        let case = Case::parse(b"puts 1\n__END__\n1\n").unwrap();
        assert_eq!(case.program, b"puts 1\n");
        let t = case.trailer.unwrap();
        assert_eq!(t.answer, answer("1\n", "", Exit::Code(0)));
        assert!(t.linux.is_none());
    }

    #[test]
    fn stderr_and_exit_have_markers() {
        let t = Trailer::parse(b"a\n#@ stderr\nwarn\n#@ exit 3\n").unwrap();
        assert_eq!(t.answer, answer("a\n", "warn\n", Exit::Code(3)));
        round_trip(&t);
    }

    #[test]
    fn an_empty_stdout_starts_at_a_marker() {
        let t = Trailer::parse(b"#@ stderr\nboom\n#@ exit 1\n").unwrap();
        assert_eq!(t.answer, answer("", "boom\n", Exit::Code(1)));
        round_trip(&t);
    }

    #[test]
    fn a_missing_final_newline_is_recorded() {
        let t = Trailer {
            answer: answer("abc", "e", Exit::Signal(9)),
            linux: None,
        };
        assert_eq!(
            String::from_utf8_lossy(&t.render()),
            "abc\n#@ nonl\n#@ stderr\ne\n#@ nonl\n#@ signal 9\n"
        );
        round_trip(&t);
    }

    #[test]
    fn a_body_line_that_looks_like_a_marker_is_escaped() {
        for body in ["#@ stderr\n", "##@ x\n", "#@\n", "###@@\n", "#x@\n"] {
            let t = Trailer {
                answer: answer(body, "", Exit::Code(0)),
                linux: None,
            };
            round_trip(&t);
        }
    }

    #[test]
    fn blank_lines_and_binary_bytes_survive() {
        let t = Trailer {
            answer: answer("\n\na\n\n", "\u{0}\u{ff}\n", Exit::Code(0)),
            linux: None,
        };
        round_trip(&t);
        let raw = Trailer {
            answer: Answer {
                stdout: vec![0xff, 0xfe, b'\n', 0x00],
                stderr: Vec::new(),
                exit: Exit::Code(0),
            },
            linux: None,
        };
        round_trip(&raw);
    }

    #[test]
    fn a_linux_answer_is_its_own_unit() {
        let t = Trailer {
            answer: answer("mac\n", "", Exit::Code(0)),
            linux: Some(answer("lin\n", "l\n", Exit::Code(2))),
        };
        assert_eq!(
            String::from_utf8_lossy(&t.render()),
            "mac\n#@ linux stdout\nlin\n#@ linux stderr\nl\n#@ linux exit 2\n"
        );
        round_trip(&t);
        let empty_linux = Trailer {
            answer: answer("mac\n", "", Exit::Code(0)),
            linux: Some(Answer::default()),
        };
        round_trip(&empty_linux);
    }

    #[test]
    fn an_unknown_marker_is_an_error() {
        assert!(Trailer::parse(b"a\n#@ stdin\n").is_err());
        assert!(Trailer::parse(b"#@ exit x\n").is_err());
        assert!(Trailer::parse(b"#@ nonl\n").is_err());
    }

    #[test]
    fn directives_are_read_and_unknown_ones_refused() {
        let d = Directives::parse(
            b"#@ args: --seed 42\n#@ env: RUBY_BOX=1 X=y\n#@ only: linux\n#@ backend: jit\n#@ pkggap\n#@ gccheck: cycle leak: 1 ring\nputs 1\n",
        )
        .unwrap();
        assert_eq!(d.args, ["--seed", "42"]);
        assert_eq!(
            d.env,
            [
                ("RUBY_BOX".to_string(), "1".to_string()),
                ("X".into(), "y".into())
            ]
        );
        assert_eq!(d.only, Some(Platform::Linux));
        assert_eq!(d.backend, Some(Backend::Jit));
        assert!(d.pkggap);
        assert_eq!(d.gccheck.as_deref(), Some("cycle leak: 1 ring"));
        assert!(Directives::parse(b"#@ expected: x\n").is_err());
        assert!(Directives::parse(b"#@ args\n").is_err());
        assert!(Directives::parse(b"#@ env: novalue\n").is_err());
        assert!(Directives::parse(b"# @ not a directive\n#@x\n").is_err());
    }

    #[test]
    fn end_without_a_trailer_and_end_at_eof() {
        assert!(Case::parse(b"puts 1\n").unwrap().trailer.is_none());
        let at_eof = Case::parse(b"puts 1\n__END__").unwrap();
        assert_eq!(at_eof.trailer, Some(Trailer::default()));
        assert!(
            Case::parse(b"x = '__END__'\n__END__x\n")
                .unwrap()
                .trailer
                .is_none()
        );
    }

    #[test]
    fn render_puts_end_on_its_own_line() {
        let t = Trailer {
            answer: answer("1\n", "", Exit::Code(0)),
            linux: None,
        };
        assert_eq!(Case::render(b"puts 1", &t), b"puts 1\n__END__\n1\n");
        let case = Case::parse(&Case::render(b"puts 1\n", &t)).unwrap();
        assert_eq!(case.trailer.as_ref(), Some(&t));
    }
}
