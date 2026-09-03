# dup: fresh unfrozen payload (mutable even when the source is
# frozen, and mutating it leaves the source untouched); clone:
# carries the frozen flag.

arr = [1].freeze
d = arr.dup
d << 2
puts d.length
puts arr.length
puts d.frozen?
puts arr.clone.frozen?
__END__
2
1
false
true
