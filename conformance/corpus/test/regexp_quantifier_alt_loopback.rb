# Imported mruby-regexp da41af3c9: keep a quantifier's loop-back after an
# alternation SPLIT is inserted in front of its atom. A greedy `\d+` compiles
# to the class plus a SPLIT looping back to it; when that class also starts
# the first alternative, compile_alt inserts the alternation SPLIT in front of
# it, shifting the class down. The loop-back target equalled the insertion
# point and used to be left pointing at the new SPLIT, so `/\d+|\w/` matched
# "1b" on "1b2c3" by falling into the alternation after one digit.

p(/\d+|\w/.match("1b2c3")[0])     # "1"
p(/a+|b/.match("aaab")[0])        # "aaa"
p(/[0-9]+|z/.match("42z")[0])     # "42"
p(/\w+|\d/.match("abc")[0])       # "abc"
p(/(\d+)|(x)/.match("99").captures) # ["99", nil]

# Quantifier kinds in front of an alternation atom still work.
p(/a*|b/.match("aaa")[0])         # "aaa"
p(/a?|b/.match("a")[0])           # "a"
p(/\d{2,}|y/.match("123y")[0])    # "123"

# Plain quantifier (no alternation) and skip-past-atom jumps unaffected.
p(/\d+/.match("12ab")[0])         # "12"
p(/(ab)+c/.match("ababc")[0])     # "ababc"
p(/colou?r/.match("color")[0])    # "color"
p(/colou?r/.match("colour")[0])   # "colour"

# Lookaround (also routes through insert_inst) still works.
p(/\d+(?=px)/.match("10px")[0])   # "10"
p(/(?<=\$)\d+/.match("$50")[0])   # "50"
