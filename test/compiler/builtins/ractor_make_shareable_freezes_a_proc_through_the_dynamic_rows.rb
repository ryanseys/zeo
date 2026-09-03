# `Ractor.make_shareable` on a Proc freezes it in place (returns the same
# object) and `shareable?` reports the frozen state -- the ostruct
# `new_ostruct_member!` shape (`nil.instance_eval { Proc.new { ... } }`
# then `::Ractor.make_shareable(proc)`), where the `::Ractor` cpath form
# exercises the runtime class-method rows rather than the static fold.
# Oracle-verified. (zeo freezes without CRuby's isolation check -- see
# ractor.rs -- so the IsolationError arm is deliberately not pinned.)

name = :a
pr = nil.instance_eval { Proc.new { name } }
r = ::Ractor.make_shareable(pr)
p r.equal?(pr)
p pr.frozen?
p ::Ractor.shareable?(pr)
p ::Ractor.shareable?(proc { 1 })
p ::Ractor.shareable?(:sym)
p ::Ractor.shareable?("mut")
__END__
true
true
true
false
true
false
