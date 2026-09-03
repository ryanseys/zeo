# `Enumerator#to_s` is `Object#to_s` -- ruby never overrides it, so an
# enumerator printed with `puts` or interpolated into a string shows its
# address, and only `inspect` describes what it iterates. zeo renders both the
# same way, from one shared row (`enumerator.rs`) and one arm in
# `value.rs`'s `to_display_string`.
#
# It reads like a nicety until a program prints an enumerator INSIDE a string
# it then parses or compares: the inspect form carries the receiver's own
# inspect, so the text grows without bound for a nested chain, where ruby's
# stays one short token.
#
# `Enumerator::Lazy#to_s` already answers the address form
# (`tests/lazy_external_iteration.rb`), so the two halves of one rule
# currently disagree with each other as well as with ruby.

def norm(s) = s.sub(/0x\h+/, "0xADDR")

e = (1..3).each
puts norm(e.to_s)
puts norm("#{e}")
puts e.inspect

c = [1, 2].each_slice(1)
puts norm(c.to_s)
puts c.inspect
__END__
#<Enumerator:0xADDR>
#<Enumerator:0xADDR>
#<Enumerator: 1..3:each>
#<Enumerator:0xADDR>
#<Enumerator: [1, 2]:each_slice(1)>
