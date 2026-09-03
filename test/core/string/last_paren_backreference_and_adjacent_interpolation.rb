# `$+` is the highest-numbered group that PARTICIPATED, skipping
# declared-but-unmatched ones (rb_reg_match_last, re.c:2093), and is nil
# when only group 0 matched -- it never reports the whole match.
#
# Backslash-continued adjacent literals parse as an InterpolatedStringNode
# whose own parts are InterpolatedStringNodes, so parts must flatten
# recursively rather than being treated as leaves.

"abc123" =~ /([a-z]+)(\d+)/
puts $1
puts $2
puts $+
"abc" =~ /([a-z]+)(\d+)?/
puts $+
"xyz" =~ /xyz/
p $+
"b" =~ /(a)|(b)|(c)/
p $+
def svg(px, inner)
  "<a width='#{px}' " \
  "height='#{px}'>#{inner}</a>"
end
def tail(n)
  "n=#{n}" \
  " done"
end
puts svg(16, "x")
puts tail(7)
__END__
abc
123
123
abc
nil
"b"
<a width='16' height='16'>x</a>
n=7 done
