//! `readline` -- the `Readline` module, an in-tree require-gated extension.
//! On a real terminal the line editor is rustyline (pure Rust, so a generated
//! program still links with bare rustc); with `Readline.input =` set or a
//! non-tty stdin the read is a plain line read, which is also what makes the
//! goldens deterministic. One process-wide history backs both the editor and
//! the `HISTORY` object ([`history`]).
//!
//! The attribute surface (word-break/quote characters, completion knobs) is
//! stored state: rustyline's completer consults the completion pieces; the
//! rest are read back verbatim, the extent of what a non-GNU-readline
//! backend can honestly do (CRuby's own libedit build is similarly partial).

pub(crate) mod history;

use crate::builtins::{arity, convert};
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

pub(super) struct RlState {
    pub(super) history: Vec<String>,
    input: Option<RubyValue>,
    output: Option<RubyValue>,
    completion_proc: RubyValue,
    pre_input_hook: RubyValue,
    completion_append_character: Option<String>,
    completion_case_fold: RubyValue,
    basic_word_break_characters: String,
    completer_word_break_characters: String,
    basic_quote_characters: String,
    completer_quote_characters: String,
    filename_quote_characters: String,
    special_prefixes: String,
    vi_mode: bool,
}

pub(super) static STATE: std::sync::LazyLock<parking_lot::Mutex<RlState>> =
    std::sync::LazyLock::new(|| {
        parking_lot::Mutex::new(RlState {
            history: Vec::new(),
            input: None,
            output: None,
            completion_proc: RubyValue::Nil,
            pre_input_hook: RubyValue::Nil,
            completion_append_character: None,
            completion_case_fold: RubyValue::Nil,
            // The oracle's defaults (ruby 4.0.5), shared by both break sets.
            basic_word_break_characters: " \t\n`><=;|&{(".to_string(),
            completer_word_break_characters: " \t\n`><=;|&{(".to_string(),
            basic_quote_characters: "\"'".to_string(),
            completer_quote_characters: "\"'".to_string(),
            filename_quote_characters: String::new(),
            special_prefixes: String::new(),
            vi_mode: false,
        })
    });

/// The terminal editor, created on FIRST tty read (never at install, so a
/// headless program pays nothing and cannot fail at startup).
static EDITOR: std::sync::LazyLock<
    parking_lot::Mutex<Option<rustyline::Editor<RlHelper, rustyline::history::DefaultHistory>>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(None));

/// The rustyline helper: completion via the stored `completion_proc`, every
/// other capability left at its default.
struct RlHelper;

impl rustyline::completion::Completer for RlHelper {
    type Candidate = rustyline::completion::Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let (proc_v, breaks) = {
            let st = STATE.lock();
            (st.completion_proc.clone(), st.completer_word_break_characters.clone())
        };
        let RubyValue::Proc(p) = proc_v else {
            return Ok((0, Vec::new()));
        };
        // The word under completion starts after the nearest break character.
        let start = line[..pos]
            .rfind(|c| breaks.contains(c))
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = RubyValue::Str(crate::string_new(line[start..pos].to_string()));
        let candidates = match p.call(&[word]) {
            Ok(RubyValue::Array(a)) => a
                .lock()
                .iter()
                .map(|v| {
                    let s = v.to_display_string();
                    rustyline::completion::Pair {
                        display: s.clone(),
                        replacement: s,
                    }
                })
                .collect(),
            // A raising or non-Array proc completes to nothing, as CRuby's.
            _ => Vec::new(),
        };
        Ok((start, candidates))
    }
}

impl rustyline::hint::Hinter for RlHelper {
    type Hint = String;
}
impl rustyline::highlight::Highlighter for RlHelper {}
impl rustyline::validate::Validator for RlHelper {}
impl rustyline::Helper for RlHelper {}

/// One line off a redirected input or a non-tty stdin -- the plain-read leg,
/// no prompt and no editor, exactly what CRuby does off a pipe. `None` at EOF.
fn plain_read(input: Option<RubyValue>) -> Result<Option<String>, Signal> {
    if let Some(io) = input {
        return match crate::dispatch::send_value(&io, crate::Symbol::intern("gets"), &[], None)? {
            RubyValue::Nil => Ok(None),
            line => Ok(Some(
                convert::to_rstr(&line)?.lock().to_utf8_lossy().trim_end_matches('\n').to_string(),
            )),
        };
    }
    let mut buf = String::new();
    let n = crate::gvl::without_gvl(|| std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut buf))
        .map_err(|e| crate::builtins::file::raise_errno(&e, "readline", "<STDIN>"))?;
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(buf.trim_end_matches('\n').to_string()))
}

/// The editor leg: rustyline over the real terminal. Eof answers `None`;
/// Ctrl-C arrives as Ruby's `Interrupt`, CRuby's behaviour.
fn editor_read(prompt: &str) -> Result<Option<String>, Signal> {
    let mut slot = EDITOR.lock();
    let editor = match &mut *slot {
        Some(e) => e,
        None => {
            let mut e = rustyline::Editor::new()
                .map_err(|e| crate::dispatch::raise_error("RuntimeError", e.to_string()))?;
            e.set_helper(Some(RlHelper));
            slot.insert(e)
        }
    };
    match editor.readline(prompt) {
        Ok(line) => Ok(Some(line)),
        Err(rustyline::error::ReadlineError::Eof) => Ok(None),
        Err(rustyline::error::ReadlineError::Interrupted) => {
            Err(crate::dispatch::raise_error("Interrupt", "Interrupt".to_string()))
        }
        Err(e) => Err(crate::dispatch::raise_error("RuntimeError", e.to_string())),
    }
}

fn str_value(s: &str) -> RubyValue {
    RubyValue::Str(crate::string_new(s.to_string()))
}

fn str_arg(v: &RubyValue) -> Result<String, Signal> {
    Ok(convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned())
}

ruby_module! {
    Readline = zeo_abi::READLINE_MODULE;

    // The one history list, as an object (see `history.rs`).
    const HISTORY = history::history_value();
    // CRuby publishes the backing library's identity here ("8.2" for GNU
    // readline, "EditLine wrapper" for libedit) -- this build's is rustyline.
    const VERSION = str_value("rustyline");
    // The stock completion procs are a GNU-readline feature; the reline-backed
    // ruby 4.x answers nil for both, and so does this.
    const FILENAME_COMPLETION_PROC = RubyValue::Nil;
    const USERNAME_COMPLETION_PROC = RubyValue::Nil;

    // `Readline.readline(prompt = "", add_hist = false)` -- one line, chomped;
    // nil at EOF. A non-empty line is appended to HISTORY when asked (an empty
    // one never is, CRuby's rule).
    def self."readline" arity -1 (_recv, args, _block) {
        arity!(args, 0..=2);
        let prompt = match args.first() {
            None | Some(RubyValue::Nil) => String::new(),
            Some(v) => str_arg(v)?,
        };
        let add_hist = !matches!(args.get(1), None | Some(RubyValue::Nil) | Some(RubyValue::Bool(false)));
        let input = STATE.lock().input.clone();
        // SAFETY: plain isatty on fd 0.
        let line = if input.is_some() || unsafe { libc::isatty(0) } == 0 {
            plain_read(input)?
        } else {
            editor_read(&prompt)?
        };
        match line {
            None => Ok(RubyValue::Nil),
            Some(text) => {
                if add_hist && !text.is_empty() {
                    STATE.lock().history.push(text.clone());
                    if let Some(e) = &mut *EDITOR.lock() {
                        let _ = e.add_history_entry(&text);
                    }
                }
                Ok(str_value(&text))
            }
        }
    }

    // Where lines come from and where the editor renders -- assignable IOs,
    // the classic API's redirection points. Stored; a set input switches
    // `readline` to the plain-read leg.
    def self."input=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().input = match &args[0] {
            RubyValue::Nil => None,
            v => Some(v.clone()),
        };
        Ok(args[0].clone())
    }
    def self."output=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().output = match &args[0] {
            RubyValue::Nil => None,
            v => Some(v.clone()),
        };
        Ok(args[0].clone())
    }

    // The completion knobs. The proc is consulted by the terminal editor's
    // completer; append character and case fold are stored state.
    def self."completion_proc" (_recv, args, _block) {
        arity!(args, 0);
        Ok(STATE.lock().completion_proc.clone())
    }
    def self."completion_proc=" (_recv, args, _block) {
        arity!(args, 1);
        if !matches!(&args[0], RubyValue::Nil | RubyValue::Proc(_)) {
            return Err(crate::builtins::type_error!("argument must respond to `call'"));
        }
        STATE.lock().completion_proc = args[0].clone();
        Ok(args[0].clone())
    }
    def self."completion_append_character" (_recv, args, _block) {
        arity!(args, 0);
        Ok(match &STATE.lock().completion_append_character {
            Some(c) => str_value(c),
            None => RubyValue::Nil,
        })
    }
    def self."completion_append_character=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().completion_append_character = match &args[0] {
            RubyValue::Nil => None,
            v => {
                // Only the FIRST character survives, as in CRuby.
                let s = str_arg(v)?;
                s.chars().next().map(|c| c.to_string())
            }
        };
        Ok(args[0].clone())
    }
    def self."completion_case_fold" (_recv, args, _block) {
        arity!(args, 0);
        Ok(STATE.lock().completion_case_fold.clone())
    }
    def self."completion_case_fold=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().completion_case_fold = args[0].clone();
        Ok(args[0].clone())
    }
    def self."pre_input_hook" (_recv, args, _block) {
        arity!(args, 0);
        Ok(STATE.lock().pre_input_hook.clone())
    }
    def self."pre_input_hook=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().pre_input_hook = args[0].clone();
        Ok(args[0].clone())
    }

    // The character-set attributes, stored and read back verbatim.
    def self."basic_word_break_characters" (_recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(&STATE.lock().basic_word_break_characters))
    }
    def self."basic_word_break_characters=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().basic_word_break_characters = str_arg(&args[0])?;
        Ok(args[0].clone())
    }
    def self."completer_word_break_characters" (_recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(&STATE.lock().completer_word_break_characters))
    }
    def self."completer_word_break_characters=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().completer_word_break_characters = str_arg(&args[0])?;
        Ok(args[0].clone())
    }
    def self."basic_quote_characters" (_recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(&STATE.lock().basic_quote_characters))
    }
    def self."basic_quote_characters=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().basic_quote_characters = str_arg(&args[0])?;
        Ok(args[0].clone())
    }
    def self."completer_quote_characters" (_recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(&STATE.lock().completer_quote_characters))
    }
    def self."completer_quote_characters=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().completer_quote_characters = str_arg(&args[0])?;
        Ok(args[0].clone())
    }
    def self."filename_quote_characters" (_recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(&STATE.lock().filename_quote_characters))
    }
    def self."filename_quote_characters=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().filename_quote_characters = str_arg(&args[0])?;
        Ok(args[0].clone())
    }
    def self."special_prefixes" (_recv, args, _block) {
        arity!(args, 0);
        Ok(str_value(&STATE.lock().special_prefixes))
    }
    def self."special_prefixes=" (_recv, args, _block) {
        arity!(args, 1);
        STATE.lock().special_prefixes = str_arg(&args[0])?;
        Ok(args[0].clone())
    }

    // Editing modes -- a stored flag; rustyline's own edit mode is configured
    // per editor, so the terminal leg honors it at first creation only.
    def self."vi_editing_mode" (_recv, args, _block) {
        arity!(args, 0);
        STATE.lock().vi_mode = true;
        Ok(RubyValue::Nil)
    }
    def self."vi_editing_mode?" (_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(STATE.lock().vi_mode))
    }
    def self."emacs_editing_mode" (_recv, args, _block) {
        arity!(args, 0);
        STATE.lock().vi_mode = false;
        Ok(RubyValue::Nil)
    }
    def self."emacs_editing_mode?" (_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(!STATE.lock().vi_mode))
    }
}
