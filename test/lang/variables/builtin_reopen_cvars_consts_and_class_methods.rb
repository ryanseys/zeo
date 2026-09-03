# Class-level state on a builtin: `@@cvar` + `CONST` in the body (ordinary
# ownership machinery, owner = the builtin's ClassId), `def self.x` via the
# generated `__bm_Array` container, and `Array::LIMIT` readable externally.

class Array
  @@made = 0
  LIMIT = 3

  def self.tally_up
    @@made = @@made + 1
    @@made
  end

  def under_limit?
    length < LIMIT
  end
end

puts Array.tally_up
puts Array.tally_up
puts [1, 2].under_limit?
puts [1, 2, 3, 4].under_limit?
puts Array::LIMIT
__END__
1
2
true
false
3
