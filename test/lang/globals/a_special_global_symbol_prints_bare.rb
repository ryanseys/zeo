# `Symbol#inspect` prints a special global's name bare, the way ruby's
# `is_special_global_name` reads it: one punctuation character from a fixed
# set, `$0`, a run of digits, or `$-` with exactly one alphanumeric after it.
# The name must END there, so `$00` and `$-ab` quote.
bare = ["$~", "$*", "$$", "$?", "$!", "$@", "$/", "$\\", "$;", "$,", "$.",
        "$=", "$:", "$<", "$>", "$\"", "$&", "$`", "$'", "$+",
        "$0", "$1", "$12", "$-w", "$-0"]
puts bare.map { |n| n.to_sym.inspect }.join(" ")

# Everything else keeps its quotes.
quoted = ["$00", "$-", "$-ab", "$^", "$#", "$[", "$%", "$ ", "$"]
puts quoted.map { |n| n.to_sym.inspect }.join(" ")

# The plain-identifier globals were already bare, and stay so.
p [:$stdout, :$_, :$a, :$LOAD_PATH]

# A bare name is still an ordinary symbol everywhere else.
p :$1.to_s, :$1.length, :$~ == "$~".to_sym
p [:$~, :$*].map(&:inspect)
p({ :$~ => 1 })
p [:$~, :$1].sort_by(&:to_s)

# A hash label is STRICTER than a bare symbol: `:+` prints bare but `{+: 1}`
# is not a literal anyone can paste back, so the shorthand quotes a leading
# `@`/`$`/`!` and a trailing operator character.
labels = ["a", "a?", "a!", "a=", "$~", "@a", "@@a", "+", "[]", "[]=", "<=>",
          "==", "!", "!~", "A", "café", "a@", "a<"]
puts labels.map { |k| { k.to_sym => 1 }.inspect }.join(" ")
p({ a: 1, "b c": 2, :$~ => 3 })
__END__
:$~ :$* :$$ :$? :$! :$@ :$/ :$\ :$; :$, :$. :$= :$: :$< :$> :$" :$& :$` :$' :$+ :$0 :$1 :$12 :$-w :$-0
:"$00" :"$-" :"$-ab" :"$^" :"$#" :"$[" :"$%" :"$ " :"$"
[:$stdout, :$_, :$a, :$LOAD_PATH]
"$1"
2
true
[":$~", ":$*"]
{"$~": 1}
[:$1, :$~]
{a: 1} {a?: 1} {a!: 1} {"a=": 1} {"$~": 1} {"@a": 1} {"@@a": 1} {"+": 1} {"[]": 1} {"[]=": 1} {"<=>": 1} {"==": 1} {"!": 1} {"!~": 1} {A: 1} {café: 1} {"a@": 1} {"a<": 1}
{a: 1, "b c": 2, "$~": 3}
