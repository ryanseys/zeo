# round(digits, half: mode): the digits branch hard-coded the default
# rounding, so the mode was silently ignored, and a mode CRuby does not know
# fell back to it instead of raising.
p(0.125.round(2, half: :even))
p(0.125.round(2, half: :down))
p(1.25.round(1, half: :even))
p((-0.125).round(2, half: :even))
v001 = 0.125; c001 = (v001.round(2, half: :even)); p c001

r002 = (1.5.round(half: :bogus) rescue $!.class); p r002
r003 = (1.5.round(2, half: :bogus) rescue $!.class); p r003
r004 = (15.round(-1, half: :bogus) rescue $!.class); p r004
p(1.5.round(half: :even))
p(2.5.round(half: :even))
p(2.5.round(half: :up))
p(2.5.round(half: :down))
p(15.round(-1, half: :even))
p(25.round(-1, half: :even))
p(0.125.round(2))
p(1.25.round(1))
