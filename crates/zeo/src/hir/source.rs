use super::*;

/// Index into `Hir::files` -- which source file a `Span` points into.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId(pub u32);

/// A byte range in one source file, straight from prism's own offsets.
/// Stamped per-NODE in `Hir::spans` (parallel to the arena, so `HirNode`
/// itself carries no span field) by the `lower_node` wrapper; a node pushed
/// outside any lowering frame -- a synthetic desugaring node, the `Program`
/// root -- gets `Span::SYNTH`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// The no-provenance sentinel: a synthetic node, or one lowered from a
    /// source string no file entry was registered for (the exception
    /// prelude, `eval` bodies). `Span::SYNTH.known()` is `None`.
    pub const SYNTH: Span = Span {
        file: FileId(u32::MAX),
        start: 0,
        end: 0,
    };

    /// `Some(self)` for a real span, `None` for `SYNTH`.
    pub fn known(self) -> Option<Span> {
        (self.file.0 != u32::MAX).then_some(self)
    }
}

/// Where a script's `__END__` DATA section starts. Ruby exposes those bytes as
/// an open `File` in `DATA`, seeked past the marker -- so what travels to the
/// runtime is a path plus an offset, not the bytes: `DATA.rewind` seeks to
/// offset 0 of the SOURCE file, which only a real handle on it can do.
///
/// The path is absolutized at compile time, since the binary can run from any
/// directory.
#[derive(Clone)]
pub struct DataSection {
    pub path: String,
    pub offset: u64,
}

/// One registered source file: the name diagnostics display (the path as
/// given, `"-e"`, ...) plus the full source text a renderer excerpts from.
#[derive(Clone)]
pub struct SourceFile {
    pub name: String,
    /// Shared rather than owned: the loader parses from this same text and
    /// must keep it alive while `ruby_prism` borrows it, so an owned `String`
    /// here forced a full copy of every Ruby file it read.
    pub source: std::sync::Arc<str>,
    /// This file's OWN `# frozen_string_literal: true` magic comment (each
    /// required file carries its own, not the entry file's).
    pub frozen_string_literal: bool,
    /// Byte offset of each line's start (`[0]` is always 0), built once at
    /// registration -- `line_at` answers in a binary search instead of
    /// re-counting newlines from byte 0, which made per-statement line
    /// stamping quadratic in file size at gem scale.
    line_starts: Vec<u32>,
    /// What to add to a line counted from this file's own start. Zero for
    /// every file on disk; a run-time `eval` snippet numbers its first
    /// line from the `line` argument CRuby gives it, and every span,
    /// frame and `__LINE__` in it must agree. Signed -- see
    /// [`crate::CompileOptions::line_offset`].
    line_offset: i32,
}

impl SourceFile {
    /// The 1-based line containing byte offset `byte`: exactly
    /// `1 + newlines strictly before byte`, the same count the old linear
    /// scan produced (a start `s = i + 1` satisfies `s <= byte` iff the
    /// newline at `i` sits strictly before `byte`).
    pub fn line_at(&self, byte: u32) -> u32 {
        let counted = self.line_starts.partition_point(|&s| s <= byte) as i64;
        // A line at or below zero is ruby's own "no line number" frame:
        // `eval(src, b, "f.rb", 0)` renders `f.rb:in '<main>'` for the
        // first line, and zero is already that marker downstream.
        (counted + i64::from(self.line_offset)).max(0) as u32
    }
}

/// A `# frozen_string_literal: true` magic comment in the leading comment
/// block (after an optional shebang). CRuby only honors it there, before any
/// code; a blank or code line ends the region.
pub fn magic_frozen_string_literal(source: &str) -> bool {
    for (i, line) in source.lines().enumerate() {
        let line = line.trim_start();
        if i == 0 && line.starts_with("#!") {
            continue;
        }
        if !line.starts_with('#') {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(idx) = lower.find("frozen_string_literal") {
            let rest = line[idx + "frozen_string_literal".len()..].trim_start();
            if let Some(rest) = rest.strip_prefix(':') {
                let value: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                return value.eq_ignore_ascii_case("true");
            }
        }
    }
    false
}

impl Hir {
    /// Registers a source file for span provenance; the caller then sets
    /// `lowering_file` while that file's statements lower.
    pub fn add_file(
        &mut self,
        name: impl Into<String>,
        source: impl Into<std::sync::Arc<str>>,
    ) -> FileId {
        self.add_file_at(name, source, 0)
    }

    /// [`add_file`](Self::add_file) for a file whose first line is not 1 --
    /// see [`SourceFile::line_offset`].
    pub fn add_file_at(
        &mut self,
        name: impl Into<String>,
        source: impl Into<std::sync::Arc<str>>,
        line_offset: i32,
    ) -> FileId {
        let source = source.into();
        let frozen_string_literal = magic_frozen_string_literal(&source);
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        self.files.push(SourceFile {
            name: name.into(),
            source,
            frozen_string_literal,
            line_starts,
            line_offset,
        });
        FileId((self.files.len() - 1) as u32)
    }
}
