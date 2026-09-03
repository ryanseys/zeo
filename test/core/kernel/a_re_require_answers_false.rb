# `require` answers true the first time and false every time after -- the
# question is whether the FEATURE loaded, not whether this call site ran.
#
# A resolvable literal require folds to a boolean at compile time, and the
# fold used to answer `true` unconditionally: it sees a NAME, and which file
# that name resolves to is the loader's knowledge, not the lowering's. So
# every re-require of a file read as a first load. (A re-require of a BUILTIN
# was already right -- that arm tracks its own activation set.)
#
# The two spellings must agree with each other as well as with themselves:
# they name one file, so the second one is a re-require whichever came first.

# Statement position, three times.
p require_relative("a_re_require_answers_false/dep")
p require_relative("a_re_require_answers_false/dep")
p require_relative("a_re_require_answers_false/dep")

# A second file's require of the same target is a re-require too.
require_relative "a_re_require_answers_false/other"

# Twice inside ONE statement, on a file nothing has loaded yet: the first
# loads, the second does not. The statement's own splice has not happened when
# the fold runs, so both call sites read the loaded set as free.
p [
  require_relative("a_re_require_answers_false/pair"),
  require_relative("a_re_require_answers_false/pair"),
]

# A runtime call answers from the loaded set, not from a fold.
p [1, 2].map { require_relative("a_re_require_answers_false/dep") }

# A builtin feature, whose re-require was already correct.
p require("set")
p require("set")

p DEP
p PAIR
__END__
dep.rb ran
true
false
false
false
pair.rb ran
[true, false]
[false, false]
false
false
:dep
:pair
