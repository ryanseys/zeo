# An ivar written on a bare heap value is filed under the value's ADDRESS,
# which is unique only while the value lives. The table holds a WEAK reference
# to every owner so the allocation -- and therefore the address -- stays
# reserved; the ivar row is dropped in the same sweep that lets the pin go.
#
# The order inside that sweep is the whole of it. Dropping the pin is what
# frees the allocation and lets the allocator hand the address out again, so
# the ivar row has to go first. Reversing the two lines makes both counts
# below read 2000: every later value of the same shape lands on a dead one's
# address and inherits its ivars. `tests/spinel/singleton_address_reuse.rb` is
# the same test for the singleton table, which made exactly that mistake.
def with_ivar
  a = [1, 2, 3]
  a.instance_variable_set(:@tag, "mine")
  a
end

def with_str_ivar
  s = "victim"
  s.instance_variable_set(:@mark, 1)
  s
end

400.times { with_ivar }
400.times { with_str_ivar }
GC.start

strays = [0, 0]
2000.times do
  strays[0] += 1 unless [9, 8, 7].instance_variables.empty?
  strays[1] += 1 unless ("x" * 6).instance_variables.empty?
end
p strays

# The live owners still answer, which is what the pin has to preserve.
p with_ivar.instance_variable_get(:@tag)
p with_str_ivar.instance_variable_get(:@mark)

# `dup` carries ivars (only singletons are clone-only), onto a fresh identity.
kept = with_ivar
p kept.dup.instance_variable_get(:@tag)
p kept.dup.equal?(kept)
__END__
[0, 0]
"mine"
1
"mine"
false
