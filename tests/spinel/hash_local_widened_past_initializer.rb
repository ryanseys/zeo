# A local the analysis widened past its initializer's hash variant. The
# variants are separate C structs, so the pointer went into the slot with no
# conversion and the C build stopped.
#
# 0b3ec5fb widened the arm that reaches this -- a block over an unresolved
# receiver has its params widened to poly -- so `sub[k] = v` widens `sub` while
# its initializer still answers Hash[String, String]. The widening is right:
# the receiver really can be either at run time. What was missing is the
# conversion, which the ARGUMENT path has had since #3998.

class A
  def each
    yield "a", "b"
  end
end

module Tep
  def self.str_hash
    Hash.new("")
  end
end

def keep
  h = Tep.str_hash
  h["k"] = "v"
  h
end

def nest(flat)
  sub = Tep.str_hash
  flat.each { |k, v| sub[k] = v }
  sub
end

puts keep.size
puts nest({ "a" => "b" }).size
puts nest(A.new).size
p nest({ "a" => "b" })
p nest(A.new)
p nest(A.new)["a"]

# the same shape into a symbol-keyed slot
module Sym
  def self.sym_hash = { x: "1" }
end

def nest_sym(flat)
  sub = Sym.sym_hash
  flat.each { |k, v| sub[k] = v }
  sub
end

p nest_sym({ y: "2" }).size
p nest_sym(A.new).size

# a hash that is NOT widened keeps its variant, which is the control: widening
# every one of these would box a key the whole program reads as a String
def plain
  h = Tep.str_hash
  h["a"] = "b"
  h["a"]
end
p plain
