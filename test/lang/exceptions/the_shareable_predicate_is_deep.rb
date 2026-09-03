# A frozen container holding an UNFROZEN element is not shareable --
# CRuby's frozen-and-everything-reachable-shareable rule.

arr = ["mut"]
arr.freeze
puts Ractor.shareable?(arr)
Ractor.make_shareable(arr)
puts Ractor.shareable?(arr)
__END__
false
true
