# `%z` ignores the `-` and `_` pad flags. zeo answers the plain `%z` string
# for all three spellings; ruby answers something else for each:
#
#     Time.at(0).utc.strftime("%-z")   ruby "-0000"   zeo "+0000"
#     Time.at(0).utc.strftime("%_z")   ruby " +000"   zeo "+0000"
#
# Ruby's answers are artifacts rather than intent -- `%z` renders through the
# same width/precision macro every numeric directive uses, so `-` (which forces
# precision 1 and clears the pad) and `_` (which pads with spaces) reach a value
# whose sign has already been folded in, and the result loses a digit or grows
# one. They are still what the oracle prints, which is what makes this a
# divergence rather than a preference.
#
# zeo's `%z` is `offset_str`, which reads neither `pad` nor `width` -- the flag
# loop parses them and the directive never asks. `%0z` and `%^z` agree by
# accident, since ruby's answer for those IS the plain form.
#
# Found by differential probe while implementing `%U`/`%W`/`%V`/`%G`; every
# other directive-and-flag pair in that sweep (129,600 cases) matches.

t = Time.at(0).utc
p [t.strftime("%z"), t.strftime("%-z"), t.strftime("%_z")]

e = Time.at(0).localtime("+05:30")
p [e.strftime("%z"), e.strftime("%-z"), e.strftime("%_z")]

w = Time.at(0).localtime("-08:00")
p [w.strftime("%z"), w.strftime("%-z"), w.strftime("%_z")]

# The colon forms and the unflagged spellings already agree.
p [t.strftime("%0z"), t.strftime("%^z"), t.strftime("%:z"), t.strftime("%::z")]
p [e.strftime("%:z"), w.strftime("%::z")]
