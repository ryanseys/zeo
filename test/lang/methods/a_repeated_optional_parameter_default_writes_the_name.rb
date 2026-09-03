# CRuby compiles an optional parameter's DEFAULT as an assignment to the NAME,
# not to the slot -- so a repeated `_`'s later default overwrites the local the
# FIRST `_` owns, which is the one every read answers. zeo bound each slot
# independently, so a later default was invisible.
def both_default(_ = 1, _ = 2) = _
p [both_default, both_default(9), both_default(9, 8)]
def req_then_opt(_, _ = 5) = _
p [req_then_opt(1), req_then_opt(1, 2)]
def lead(a, _ = 2, _ = 3) = [a, _]
p [lead(0), lead(0, 7), lead(0, 7, 8)]
def three(_ = 1, _ = 2, _ = 3) = _
p [three, three(9), three(9, 8), three(9, 8, 7)]
# a default that reads an earlier parameter
def reads(a, _ = a, _ = a + 1) = [a, _]
p [reads(5), reads(5, 6), reads(5, 6, 7)]
# named (non-underscore) duplicates are a SyntaxError in ruby, so only `_`
# shapes exist. A single optional is unchanged.
def single(a = 4) = a
p [single, single(1)]
__END__
[2, 2, 9]
[5, 1]
[[0, 3], [0, 3], [0, 7]]
[3, 3, 3, 9]
[[5, 6], [5, 6], [5, 6]]
[4, 1]
