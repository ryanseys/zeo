//! The text a `SyntaxError` carries -- prism's own rich report, ported.
//!
//! A snippet that does not parse raises a `SyntaxError` whose message a
//! Ruby program can read, print and match on, so the message is part of
//! the observable surface rather than a compiler nicety. CRuby builds it
//! with `pm_errors_format` (`prism/errors_format.c`), which prints a
//! header, the offending line with two lines of context, and one caret
//! row per diagnostic; this is that function's PLAIN branch, term for
//! term, over the pieces the `ruby-prism` crate exposes.
//!
//! The CLI renders a program's parse failure through miette instead --
//! same failure, a different reader. See `diagnostics::LowerDiagnostic`.

use ruby_prism::ParseResult;

/// How far past an error prism keeps the line before it elides.
const TRUNCATE: usize = 30;

/// One diagnostic, placed: the line it sits on and the byte columns it
/// spans within that line.
struct Rich {
    message: String,
    line: i32,
    column_start: usize,
    column_end: usize,
    idx: usize,
}

/// The byte offset each line starts at. The last entry equals the source
/// length exactly when the source ends in a newline -- which is the test
/// prism's own trailing-context loop makes.
fn line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    offsets.extend(
        source
            .bytes()
            .enumerate()
            .filter(|&(_, b)| b == b'\n')
            .map(|(i, _)| i + 1),
    );
    offsets
}

/// `(line, byte column within it)` for a byte offset.
fn line_column(offsets: &[usize], off: usize, start_line: i32) -> (i32, usize) {
    let idx = offsets.partition_point(|&o| o <= off).saturating_sub(1);
    (start_line + idx as i32, off - offsets[idx])
}

/// The gutter widths prism picks from the widest line number it will print.
fn gutter(first_line: i32, last_line: i32) -> (usize, String) {
    let widen = |n: i32| if n < 0 { (-n) * 10 } else { n };
    let max_line = widen(first_line).max(widen(last_line));
    let width = match max_line {
        ..10 => 1,
        10..100 => 2,
        100..1000 => 3,
        1000..10000 => 4,
        _ => 5,
    };
    // `  ~~~~~` at width 1, one more tilde per column, capped at width 4's.
    let tildes = 5 + width.min(4) - 1;
    (width, format!("  {}\n", "~".repeat(tildes)))
}

/// The character width of the byte at `off`, as prism's UTF-8 encoding
/// answers it -- 1 for anything it cannot decode, which is what keeps the
/// caret walk terminating.
fn char_width(source: &str, off: usize) -> usize {
    let bytes = source.as_bytes();
    match bytes.get(off) {
        None => 1,
        Some(&b) if b < 0x80 => 1,
        Some(&b) if b >= 0xf0 => 4,
        Some(&b) if b >= 0xe0 => 3,
        Some(&b) if b >= 0xc0 => 2,
        Some(_) => 1,
    }
}

/// What every rendered line shares: the source, its line offsets, the gutter
/// width, and the line number the first offset stands for.
struct Listing<'a> {
    source: &'a str,
    offsets: &'a [usize],
    width: usize,
    start_line: i32,
}

/// One source line, with prism's start/end elision applied.
fn format_line(
    out: &mut String,
    l: &Listing<'_>,
    line: i32,
    column_start: usize,
    column_end: usize,
) {
    let (source, offsets, width, start_line) = (l.source, l.offsets, l.width, l.start_line);
    let index = (line - start_line) as usize;
    let Some(&line_start) = offsets.get(index) else {
        return;
    };
    let mut start = line_start;
    let mut end = offsets.get(index + 1).copied().unwrap_or(source.len());
    out.push_str(&format!("{line:width$} | "));
    let mut truncate_end = false;
    if column_end != 0 && (end - start).saturating_sub(column_end) >= TRUNCATE {
        let mut candidate = start + column_end + TRUNCATE;
        let mut cursor = start;
        while cursor < candidate {
            let w = char_width(source, cursor);
            if cursor + w > candidate {
                candidate = cursor;
                break;
            }
            cursor += w;
        }
        end = candidate;
        truncate_end = true;
    }
    if column_start >= TRUNCATE {
        out.push_str("... ");
        start += column_start;
    }
    out.push_str(&source[start.min(source.len())..end.min(source.len())]);
    if truncate_end {
        out.push_str(" ...\n");
    } else if end == source.len() && end > 0 && source.as_bytes()[end - 1] != b'\n' {
        out.push('\n');
    }
}

/// The body of the report: every error in source order, each under the
/// line it sits on, with up to two lines of context either side.
fn rich_report(
    out: &mut String,
    source: &str,
    offsets: &[usize],
    start_line: i32,
    errors: &[Rich],
    inline_messages: bool,
) {
    let (width, divider) = gutter(errors[0].line, errors[errors.len() - 1].line);
    let listing = Listing {
        source,
        offsets,
        width,
        start_line,
    };
    let blank = format!("  {} | ", " ".repeat(width));
    let mut last_line = start_line - 1;
    let mut last_column_start = 0usize;
    for idx in 0..errors.len() {
        let e = &errors[idx];
        if e.line - last_line > 1 {
            if e.line - last_line > 2 {
                if idx != 0 && e.line - last_line > 3 {
                    out.push_str(&divider);
                }
                out.push_str("  ");
                format_line(out, &listing, e.line - 2, 0, 0);
            }
            out.push_str("  ");
            format_line(out, &listing, e.line - 1, 0, 0);
        }
        if idx == 0 || e.line != last_line {
            out.push_str("> ");
            last_column_start = e.column_start;
            let mut column_end = e.column_end;
            for next in &errors[idx + 1..] {
                if next.line != e.line {
                    break;
                }
                column_end = column_end.max(next.column_end);
            }
            format_line(out, &listing, e.line, e.column_start, column_end);
        }
        let line_start = offsets[(e.line - start_line) as usize];
        if line_start == source.len() {
            out.push('\n');
        }
        out.push_str(&blank);
        let mut column = 0usize;
        if last_column_start >= TRUNCATE {
            out.push_str("    ");
            column = last_column_start;
        }
        while column < e.column_start {
            out.push(' ');
            column += char_width(source, line_start + column);
        }
        out.push('^');
        column += char_width(source, line_start + column);
        while column < e.column_end {
            out.push('~');
            column += char_width(source, line_start + column);
        }
        if inline_messages {
            out.push(' ');
            out.push_str(&e.message);
        }
        out.push('\n');
        last_line = e.line;
        let next_line = match errors.get(idx + 1) {
            Some(next) => next.line,
            None => {
                let mut n = offsets.len() as i32 + start_line;
                if offsets[offsets.len() - 1] == source.len() {
                    n -= 1;
                }
                n
            }
        };
        for _ in 0..2 {
            if next_line - last_line <= 1 {
                break;
            }
            last_line += 1;
            out.push_str("  ");
            format_line(out, &listing, last_line, 0, 0);
        }
    }
}

/// The whole `SyntaxError` message for a parse that failed: prism's own
/// text, which is what a Ruby program sees from CRuby.
///
/// `start_line` is the line the source's FIRST line reports -- `eval`'s
/// fourth argument, so a snippet's rows carry the caller's numbering.
/// `skip` drops the diagnostics the caller's parse context makes legal.
pub(crate) fn report(
    source: &str,
    result: &ParseResult<'_>,
    file: &str,
    start_line: i32,
    skip: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let offsets = line_offsets(source);
    let mut errors: Vec<Rich> = Vec::new();
    for (idx, d) in result.errors().filter(|e| !skip(e.message())).enumerate() {
        let loc = d.location();
        let (line, column_start) = line_column(&offsets, loc.start_offset(), start_line);
        let (end_line, end_column) = line_column(&offsets, loc.end_offset(), start_line);
        let mut column_end = if end_line == line {
            end_column
        } else {
            let i = (line - start_line) as usize;
            offsets[i + 1] - offsets[i] - 1
        };
        if column_start == column_end {
            column_end += 1;
        }
        errors.push(Rich {
            message: d.message().to_string(),
            line,
            column_start,
            column_end,
            idx,
        });
    }
    let first = errors.first()?;
    let plural = if errors.len() > 1 { "s" } else { "" };
    let mut out = format!("{file}:{}: syntax error{plural} found\n", first.line);
    // Source order for display; a tie on position puts the LAST-generated
    // diagnostic first, which is prism's own comparator.
    errors.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then(a.column_start.cmp(&b.column_start))
            .then(b.idx.cmp(&a.idx))
    });
    rich_report(&mut out, source, &offsets, start_line, &errors, true);
    Some(out)
}
