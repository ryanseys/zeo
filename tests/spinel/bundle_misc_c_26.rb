# Bundled tests:
#   - regexp
#   - regexp_backspace_in_class
#   - regexp_inline_flag_group_no_hang

# === regexp ===
def t_regexp
  # Test Regexp support
  
  # match operator
  if "hello 123 world" =~ /\d+/
    puts "matched"
  end
  
  # match with capture
  str = "2024-03-17"
  if str =~ /(\d{4})-(\d{2})-(\d{2})/
    puts $1  # 2024
    puts $2  # 03
    puts $3  # 17
  end
  
  # String#match?
  puts "hello".match?(/ell/)    # true
  puts "hello".match?(/xyz/)    # false
  
  # String#gsub with regexp
  puts "hello world".gsub(/o/, "0")   # hell0 w0rld
  
  # String#sub with regexp
  puts "hello world".sub(/o/, "0")    # hell0 world
  
  # String#scan
  "one 1 two 2 three 3".scan(/\d+/) do |m|
    puts m
  end
  # 1, 2, 3
  
  # String#split with regexp
  parts = "a, b,  c".split(/,\s*/)
  puts parts.length  # 3
end
t_regexp

# === regexp_backspace_in_class ===
def t_regexp_backspace_in_class
  # `\b` inside a `[...]` character class means U+0008 (backspace),
  # not the letter `b`. Outside `[...]`, `\b` is a word-boundary
  # anchor — the regex compiler's outer loop consumes that case
  # before reaching parse_escape, so adding the 0x08 mapping in
  # parse_escape only fires for the inside-class meaning. Pre-fix
  # spinel treated `[\b]` as `[b]`, stripping the letter b from any
  # `gsub(/[\b]/, ...)`. Issue #632.
  
  puts "Ruby".gsub(/[\b]/, "X")
  puts "a\bc".gsub(/[\b]/, "X")
  puts "no backspace here".gsub(/[\b]/, "X")
  puts "back\bspace".gsub(/[\b]/, "X")
  
  # `\b` outside a character class stays a word boundary.
  puts "hello world".gsub(/\bworld\b/, "earth")
  puts "hello worldwide".gsub(/\bworld\b/, "earth")
  
  # Combined char class with `\b` plus other escapes.
  puts "a\tb\nc\bd".gsub(/[\t\n\b]/, "_")
end
t_regexp_backspace_in_class

# === regexp_inline_flag_group_no_hang ===
def t_regexp_inline_flag_group_no_hang
  # `(?xim:...)` inline-flag groups previously parsed into an infinite
  # loop in re_compile: the `?` after `(` didn't match any recognized
  # directive (`:`, `=`, `!`, `<...`) and didn't advance c->p, so
  # compile_seq's outer loop spun forever on the unconsumed `?`.
  # Sam Ruby's #600 puzzle 3 (`p(/(?x:foo)/.to_s)`) hung at runtime
  # during the sp_re_init's static-regex compilation.
  #
  # Fix: when the `(?` lookahead matches a recognized flag char
  # (x / i / m / s / u / a), consume to `:` (non-capturing body)
  # or `)` (whole-group flag application -- spinel doesn't track
  # scoped flag state, so the directive is consumed without
  # emitting). Unrecognized `(?<X>` now raises a clean
  # `unrecognized (? construct` compile_error instead of hanging.
  #
  # Semantically /x's whitespace-stripping IS NOT applied inside
  # the sub-pattern -- spinel's compile-time flag handling only
  # does top-level stripping. Patterns whose `/x` flag is decorative
  # (whitespace inside `(?x:body)` for layout only) match the same
  # as the spinel literal would; patterns relying on the strip
  # would behave differently, but no longer hang.
  
  # `(?x:...)` with no whitespace in body. /foo/ matches "foo".
  puts (/(?x:foo)/ =~ "foobar").to_s     # 0
  puts (/(?x:foo)/ =~ "barbaz").nil?     # true
  
  # `(?:...)` non-capturing still works.
  puts (/(?:hello) (?:world)/ =~ "hello world").to_s  # 0
  
  # `(?i:...)` consumes the flag; spinel doesn't honor case-insensitivity
  # inside the group, but the parse no longer hangs.
  puts (/(?i:abc)/ =~ "abc").to_s       # 0
  
  # `(?m:...)` similar.
  puts (/(?m:foo)/ =~ "foo").to_s       # 0
end
t_regexp_inline_flag_group_no_hang

