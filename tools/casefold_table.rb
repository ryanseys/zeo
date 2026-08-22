# frozen_string_literal: true

# Generates the FULL case-folding table from the ruby 4.0.6 oracle:
#
#   crates/zeo-rt/src/enc/casefold.rs
#
# Run as `mise exec ruby@4.0.6 -- ruby tools/casefold_table.rb`. The file is
# checked in; regenerate it rather than editing by hand.
#
# Only the codepoints whose FOLD differs from their lowercase are listed --
# everything else folds by `String#downcase`, which Rust's `to_lowercase`
# already answers identically. That is 297 rows rather than several thousand,
# and it is what `String#downcase(:fold)` needs on top of the ordinary map.

ROOT = File.expand_path("..", __dir__)

rows = []
(0..0x10FFFF).each do |cp|
  next if cp.between?(0xD800, 0xDFFF)
  s = begin
    cp.chr(Encoding::UTF_8)
  rescue RangeError
    next
  end
  fold = s.downcase(:fold)
  rows << [cp, fold] if fold != s.downcase
end

out = +<<~HEAD
  //! Full case folding, generated from the ruby #{RUBY_VERSION} oracle by
  //! `tools/casefold_table.rb`. Do not edit by hand.
  //!
  //! Only the codepoints whose FOLD differs from their lowercase are here:
  //! everything else folds exactly as `String#downcase` maps it. `\u{00df}`
  //! (`\u{00df}` -> `"ss"`) and the ligatures are the shape that makes the
  //! difference observable -- a fold can be LONGER than its source.

  /// `(codepoint, folded)`, sorted by codepoint for a binary search.
  pub(crate) const FOLD_EXCEPTIONS: &[(char, &str)] = &[
HEAD
rows.each do |cp, fold|
  escaped = fold.codepoints.map { |c| format("\\u{%x}", c) }.join
  out << format("    ('\\u{%x}', \"%s\"),\n", cp, escaped)
end
out << "];\n"
File.write(File.join(ROOT, "crates/zeo-rt/src/enc/casefold.rs"), out)
puts "wrote crates/zeo-rt/src/enc/casefold.rs (#{rows.size} rows)"
