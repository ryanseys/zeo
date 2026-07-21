# Silent-elision regression: `<str_str_hash?>[k] = v` previously
# fell through every arm of compile_bracket_assign with no emit, so
# the assignment vanished. The two missing entry points were the
# `str_str_hash` / `str_int_hash` base arms (mirror of the existing
# `sym_*_hash` / `int_str_hash` arms), plus the nullable-pointer
# strip at the top so `str_str_hash?` reaches the same emit.
# Issue #610: roundhouse's `dispatch_request` builds a merged
# str_str_hash from a path-params dup (typed str_str_hash? because
# the source ivar is nullable) and then writes into it inside a
# `.each` block; pre-fix the entire merge loop body disappeared.

class HashHolder
  def initialize
    @h = nil
  end

  attr_accessor :h
end

def fill(holder)
  # Force the slot's static type to `str_str_hash?` by routing the
  # initial value through a method that returns the nullable shape.
  holder.h = path_params
  holder.h["c"] = "3"
  holder.h["d"] = "4"
end

def path_params
  { "a" => "1", "b" => "2" }
end

hh = HashHolder.new
fill(hh)
m = hh.h
puts m["a"]   # 1
puts m["b"]   # 2
puts m["c"]   # 3
puts m["d"]   # 4
puts m.length # 4
