# Two strftime divergences left after the flag work, both about the part of a
# directive that is NOT a flag.
#
# 1. An explicit WIDTH is ignored by every non-numeric directive. Ruby pads a
#    string directive to the field width like any other; zeo's `num_pad` is the
#    only reader of `width`, so `%10a` answers the bare day name.
#
#        Time.at(0).utc.strftime("%10a")   ruby "       Thu"   zeo "Thu"
#
#    Numeric directives already honour width (`%5Y` is `01970` in both), which
#    is what makes this specifically the string half.
#
# 2. `%v` (the VMS date, `%e-%^b-%4Y`) is not implemented, so it emits verbatim.
#
# A width after a COLON flag is a third shape: ruby rejects `%:5z` outright and
# emits it verbatim, where zeo renders the offset and drops the width.
#
# Found by a 13,800-case directive x flag x width sweep while fixing
# `tests/strftime_zone_offset_flags.rb`; every case NOT in the three shapes
# above matches, including all 84 `%z` flag spellings.

t = Time.at(0).utc

# Width on a string directive.
p [t.strftime("%10a"), t.strftime("%10A"), t.strftime("%10b"), t.strftime("%10p")]
p [t.strftime("%5u"), t.strftime("%5n").inspect, t.strftime("%3w")]
p [t.strftime("%10z"), t.strftime("%10Z")]

# The VMS date.
p t.strftime("%v")
p t.strftime("%^v")

# A width after a colon flag is invalid, and stays verbatim.
p [t.strftime("%:5z"), t.strftime("%::1z")]

# Already correct: width on a numeric directive, and every flag spelling.
p [t.strftime("%5Y"), t.strftime("%-5Y"), t.strftime("%_5Y"), t.strftime("%05Y")]
p [t.strftime("%z"), t.strftime("%-z"), t.strftime("%_z"), t.strftime("%:z")]
__END__
["       Thu", "  Thursday", "       Jan", "        AM"]
["00004", "\"    \\n\"", "004"]
["+000000000", "       UTC"]
" 1-JAN-1970"
" 1-JAN-1970"
["%:5z", "%::1z"]
["01970", "1970", " 1970", "01970"]
["+0000", "-0000", " +000", "+00:00"]
