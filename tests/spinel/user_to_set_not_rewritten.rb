# `enum.to_set` desugars to `Set.new(enum.to_a)`, but a class that defines its
# OWN to_set has to keep it. The rewrite used to run before class collection,
# so it could not tell, and rewrote every to_set in the program: a method
# returning a Hash of pending writes became a Set, and the error landed on
# whatever the real return type supported, several lines below the call.
require "set"

class Jar
  def initialize
    @out = {}
  end
  def []=(k, v)
    @out[k] = v
    v
  end
  def to_set          # NOT Enumerable#to_set
    @out
  end
end

jar = Jar.new
jar["a"] = "1"
jar["b"] = "2"
h = jar.to_set
p h.keys
p h["b"]
p h.class.to_s

# ... while a real Enumerable receiver in the SAME program still desugars
s = [3, 1, 3, 2].to_set
p s.class.to_s
p s.size
p s.include?(3)
p (1..4).to_set.size
