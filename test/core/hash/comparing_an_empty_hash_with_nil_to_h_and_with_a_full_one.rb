# `nil.to_h == {}` holds; a hash with a pair is not equal either way round.
# (spinel issue #3040)
p(nil.to_h == {})
p({} == nil.to_h)
p({} == {})
h = {"a" => 1}
p(h == {})
p({} == h)
p({}.eql?(nil.to_h))
p({} != nil.to_h)
__END__
true
true
true
false
false
true
false
