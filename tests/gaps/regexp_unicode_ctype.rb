# A POSIX bracket and a word boundary read the character types above ASCII off
# lib/regexp/re_ctype.h. Before the table, `[[:alpha:]]` held the ASCII letters
# and nothing above them and its negation held everything above them, so both
# polarities answered wrongly on text that is not ASCII; `\b` and `\B` took one
# byte, and no byte of a multi-byte character is a word character, so /\b/
# found no boundary in a subject holding no ASCII.
# (ported from mruby-regexp 55b6deab4 and 5ffcc0034)

# a boundary sits beside any script
p "日本語" =~ /\b/
p "日本語".scan(/\b/).size
p "日本語" =~ /\B/
p "こんにちは world" =~ /\bworld\b/
p "café" =~ /\bcafé\b/
p "あa" =~ /a\b/
p "λx" =~ /\b/
p "日本語です" =~ /\b語/

# what a bracket holds above ASCII, in both polarities
p "日本語" =~ /[[:word:]]/
p "日本語" =~ /[[:alpha:]]/
p "aあx" =~ /[[:^alpha:]]x/
p "１２３" =~ /[[:digit:]]+/
p "Ω" =~ /[[:upper:]]/
p "ω" =~ /[[:lower:]]/
p "　" =~ /[[:space:]]/
p "。" =~ /[[:punct:]]/
p "aあ" =~ /[[:^ascii:]]/
p "abc" =~ /[[:^ascii:]]/

# `\w` is ASCII in Ruby's syntax and stays so, which is what the boundary
# reading the wider set is measured against
p "日本語" =~ /\w/
p "日本語" =~ /\W/

# a bracket under /i reads the type of every character sharing the folding
p "Ā" =~ /[[:lower:]]/i
p "ā" =~ /[[:upper:]]/i

# a byte that starts no character is a byte, not the character it would spell
# inside one: the lone 0xB5 is that byte, where "\xC2\xB5" is µ
p "\xB5".b =~ /\b/
p "\xB5".b =~ /[[:word:]]/
p "\xC2\xB5".b =~ /[[:word:]]/
p "µ" =~ /[[:word:]]/
p "\xC2\xB5".b.scan(/\b/).size
p "µ".scan(/\b/).size

# ASCII is what it was
p "hello world" =~ /\bworld\b/
p "hello".scan(/\b/).size
p "abc123" =~ /[[:^digit:]]+/
p "a_b" =~ /\A[[:word:]]+\z/
p "[[:alpha:]]" =~ /\A\[\[:alpha:\]\]\z/
p "x" =~ /[[:xdigit:]]/
p "あ" =~ /[[:xdigit:]]/
