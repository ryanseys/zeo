# frozen_string_literal: true

# Generates the printability ranges `String#inspect` uses, from the ruby
# 4.0.6 oracle:
#
#   crates/zeo-rt/src/enc/printable.rs
#
# Run as `mise exec ruby@4.0.6 -- ruby tools/inspect_printable.rb`. The file
# is checked in; regenerate it rather than editing by hand.
#
# CRuby asks Oniguruma's `ONIGENC_IS_CODE_PRINT`, whose answer follows no
# rule the general category alone gives: U+E000 (private use) and U+00AD
# (soft hyphen) print RAW, U+FFFE (a noncharacter) and every unassigned
# codepoint escape. Asking the oracle per codepoint is the only honest way
# to get it, so that is what this does.
#
# ASCII is not in the table: `push_inspect_char` decides it before the
# lookup.

ROOT = File.expand_path("..", __dir__)

ranges = []
start = nil
(0x80..0x10FFFF).each do |cp|
  next if cp.between?(0xD800, 0xDFFF)
  s = cp.chr(Encoding::UTF_8)
  escaped = s.inspect != %("#{s}")
  if escaped
    start ||= cp
  elsif start
    ranges << [start, cp - 1]
    start = nil
  end
end
ranges << [start, 0x10FFFF] if start

out = +<<~HEAD
  //! Which codepoints `String#inspect` ESCAPES, generated from the ruby
  //! #{RUBY_VERSION} oracle by `tools/inspect_printable.rb`. Do not edit by
  //! hand.
  //!
  //! CRuby asks Oniguruma's `ONIGENC_IS_CODE_PRINT`, and its answer follows
  //! no rule the general category alone gives: U+E000 (private use) and
  //! U+00AD (soft hyphen) print RAW while U+FFFE and every unassigned
  //! codepoint escape. The table is the oracle's own answers.

  /// Inclusive `(lo, hi)` ranges of escaped codepoints, sorted and disjoint.
  /// ASCII is absent -- `push_inspect_char` decides it before the lookup.
  const ESCAPED: &[(u32, u32)] = &[
HEAD
ranges.each { |lo, hi| out << format("    (0x%X, 0x%X),\n", lo, hi) }
out << <<~TAIL
  ];

  /// Whether `String#inspect` renders this codepoint as `\\uXXXX` rather than
  /// as itself.
  pub(crate) fn escapes(cp: u32) -> bool {
      ESCAPED
          .binary_search_by(|&(lo, hi)| {
              if cp < lo {
                  std::cmp::Ordering::Greater
              } else if cp > hi {
                  std::cmp::Ordering::Less
              } else {
                  std::cmp::Ordering::Equal
              }
          })
          .is_ok()
  }
TAIL
File.write(File.join(ROOT, "crates/zeo-rt/src/enc/printable.rs"), out)
puts "wrote crates/zeo-rt/src/enc/printable.rs (#{ranges.size} ranges)"
