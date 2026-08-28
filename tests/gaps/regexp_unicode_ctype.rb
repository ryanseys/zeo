p "日本語" =~ /\b/
p "日本語".scan(/\b/).size
p "日本語" =~ /\B/
p "こんにちは world" =~ /\bworld\b/
p "café" =~ /\bcafé\b/
p "あa" =~ /a\b/
p "λx" =~ /\b/
p "日本語です" =~ /\b語/

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

p "日本語" =~ /\w/
p "日本語" =~ /\W/

p "Ā" =~ /[[:lower:]]/i
p "ā" =~ /[[:upper:]]/i

p "\xB5".b =~ /\b/
p "\xB5".b =~ /[[:word:]]/
p "\xC2\xB5".b =~ /[[:word:]]/
p "µ" =~ /[[:word:]]/
p "\xC2\xB5".b.scan(/\b/).size
p "µ".scan(/\b/).size

p "hello world" =~ /\bworld\b/
p "hello".scan(/\b/).size
p "abc123" =~ /[[:^digit:]]+/
p "a_b" =~ /\A[[:word:]]+\z/
p "[[:alpha:]]" =~ /\A\[\[:alpha:\]\]\z/
p "x" =~ /[[:xdigit:]]/
p "あ" =~ /[[:xdigit:]]/
