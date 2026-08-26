# `Array#uniq` confirms hash-equal candidates with `eql?`.
#
# The whole key table used to collapse on the user `hash` alone, so any
# two objects with colliding hashes were one key however their `eql?`
# read. `uniq` was the visible symptom; `a_user_hash_that_collides.rb`
# carries the rest, including the Hash that read back another key's value.
c = Struct.new(:v) do
  def hash = 0
  def eql?(o) = v == o.v
end
p [c.new(1), c.new(1), c.new(2)].uniq.length
p [c.new(1), c.new(2)].uniq { |x| x }.length
