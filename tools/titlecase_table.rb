# frozen_string_literal: true

# Generates the TITLECASE deltas from the ruby 4.0.6 oracle:
#
#   crates/zeo-rt/src/enc/titlecase.rs
#
# Run as `mise exec ruby@4.0.6 -- ruby tools/titlecase_table.rb`. The file is
# checked in; regenerate it rather than editing by hand.
#
# Titlecase is the THIRD of Unicode's case-mapping triple, and Rust's standard
# library has no mapping for it -- `to_uppercase` answers the full-uppercase
# form, which is right for `upcase` and wrong for `capitalize`. Only a handful
# of characters have a titlecase distinct from their uppercase (the Dz/Lj/Nj
# digraphs and the Greek iota-subscript forms), so the table is a DELTA:
# everything absent from it titlecases exactly as it uppercases.
#
# `swapcase` needs its own row, because swapping a titlecase character is
# MULTI-character: `Dz` (U+01F2) swaps to `dZ`, not to one codepoint. That is
# why the second column is a String rather than a char.

ROOT = File.expand_path("..", __dir__)

title = []
swap = []
(0..0x10FFFF).each do |cp|
  next if cp.between?(0xD800, 0xDFFF)
  s = begin
    cp.chr(Encoding::UTF_8)
  rescue RangeError
    next
  end
  # `capitalize` on a ONE-character string IS its titlecase.
  t = s.capitalize
  title << [cp, t] if t != s.upcase
  sw = s.swapcase
  # Only where swapping differs from the ordinary "up if down, down if up"
  # answer the case tables already give.
  expected = s == s.downcase ? s.upcase : s.downcase
  swap << [cp, sw] if sw != expected
end

def rows(pairs)
  pairs.map do |cp, text|
    escaped = text.codepoints.map { |c| format("\\u{%x}", c) }.join
    format("    ('\\u{%x}', \"%s\"),\n", cp, escaped)
  end.join
end

out = +<<~HEAD
  //! Titlecase deltas, generated from the ruby #{RUBY_VERSION} oracle by
  //! `tools/titlecase_table.rb`. Do not edit by hand.
  //!
  //! Titlecase is the THIRD of Unicode's case-mapping triple, and Rust's
  //! standard library has no mapping for it: `to_uppercase` answers the
  //! full-uppercase form, which is right for `upcase` and wrong for
  //! `capitalize`. Only the characters whose titlecase DIFFERS from their
  //! uppercase are here -- everything else titlecases as it uppercases.
  //!
  //! `swapcase` needs its own table because swapping a titlecase character
  //! is MULTI-character (`\u{01f2}` -> `"\u{64}\u{5a}"`), which no per-char
  //! mapping can express.

  /// `(codepoint, titlecased)`, sorted by codepoint for a binary search.
  pub(crate) const TITLECASE: &[(char, &str)] = &[
HEAD
out << rows(title)
out << "];\n\n"
out << <<~HEAD2
  /// `(codepoint, swapped)` for the characters whose swap is not simply the
  /// other case -- the titlecase forms, whose halves swap independently.
  pub(crate) const SWAPCASE_EXCEPTIONS: &[(char, &str)] = &[
HEAD2
out << rows(swap)
out << "];\n"
File.write(File.join(ROOT, "crates/zeo-rt/src/enc/titlecase.rs"), out)
puts "wrote crates/zeo-rt/src/enc/titlecase.rs " \
     "(#{title.size} titlecase, #{swap.size} swapcase rows)"
