# A regex reached through a NAME is as statically visible as the literal
# itself, and every resolver has to agree about that.
#
# re_lit_index followed a constant and a local; re_lit_src followed only a
# constant; the analyze rule looked only at a direct literal node. So a
# capturing pattern named by a constant made codegen emit the capturing scan
# under a str_array type (a PolyArray walked as a StrArray -- a segfault),
# while the same pattern named by a local silently dropped the capture groups.
# One resolver now answers for all of them.

PAT = /(\d)(\w)/
NOCAP = /\d/
FROZEN = /(\d)(\w)/.freeze

lpat = /(\d)(\w)/
lnocap = /\d/

p "a1b2".scan(/(\d)(\w)/)
p "a1b2".scan(PAT)
p "a1b2".scan(lpat)
p "a1b2".scan(FROZEN)
p "a1b2".scan(/\d/)
p "a1b2".scan(NOCAP)
p "a1b2".scan(lnocap)

# the block form destructures a capture row through a name too
"a1b2".scan(PAT) { |a, b| p [a, b] }
"a1b2".scan(lpat) { |a, b| p [a, b] }
"a1b2".scan(NOCAP) { |m| p m }
"a1b2".scan(lnocap) { |m| p m }

# resolving a name must not change what the other regex methods see
p "a1b2".sub(PAT, "-")
p "a1b2".gsub(lnocap, "#")
p "a1b2".match(PAT)[2]
p("a1b2" =~ NOCAP)
