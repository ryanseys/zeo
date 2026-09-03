# An unknown POSIX class name is a RegexpError (invalid POSIX bracket type),
# while valid ones compile; an unterminated char class reports onig's own
# wording -- "premature end of char-class", not the engine's raw multiline
# parse error and not a paraphrase of it.

p "Hi 12".scan(/[[:alpha:]]+/)
r = (begin; Regexp.new("[[:bogus:]]"); "ok"; rescue RegexpError => e; e.message; end); p r
r2 = (begin; Regexp.new("[invalid"); "ok"; rescue RegexpError => e; e.message; end); p r2
p Regexp.new("hello").match?("say hello")
__END__
["Hi"]
"invalid POSIX bracket type: /[[:bogus:]]/"
"premature end of char-class: /[invalid/"
true
