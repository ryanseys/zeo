# Four `strftime` directives are not implemented and pass through as their own
# literal text -- `%U`, `%W`, `%V` and `%G` -- so a format string asking for a
# week number gets the two characters back instead:
#
#     Time.at(1_700_000_000).utc.strftime("%U %W %V %G")
#     ruby "46 46 46 2023"      zeo "%U %W %V %G"
#
# Emitting the directive verbatim is the worst available failure: nothing
# raises, and a log line or a filename silently contains "%U" forever. `%V`/%G`
# are the ISO week-date pair, which is what a weekly rollup or a report bucket
# keys on.
#
# Two flag bugs ride along: `%0e` ignores its zero-pad request (`" 1"` for
# `"01"`), and `%#A` swapcases rather than upcases, turning "Thursday" into
# "tHURSDAY" instead of "THURSDAY".
#
# Everything else in the probe agrees, including `%j`, `%a`, `%b`, the `%D`/
# `%F`/`%T`/`%R`/`%r`/`%c`/`%x`/`%X` combinations and the `%-`/`%_`/`%^` flags.

t0 = Time.at(0).utc
p [t0.strftime("%U"), t0.strftime("%W"), t0.strftime("%V"), t0.strftime("%G")]
p Time.at(1_700_000_000).utc.strftime("%U %W %V %G")

p [t0.strftime("%0e"), t0.strftime("%#A")]

# Already correct -- a fix must not disturb these.
p [t0.strftime("%-d"), t0.strftime("%_d"), t0.strftime("%^a")]
p [t0.strftime("%D"), t0.strftime("%F"), t0.strftime("%T"), t0.strftime("%R")]
p Time.at(1_700_000_000).utc.strftime("%j %a %b %Y")
