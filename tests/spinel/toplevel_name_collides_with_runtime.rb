# A top-level method mangles to `sp_<name>`, which is the runtime's own
# namespace: `def str_hash` collided with sp_str_hash and the generated C did
# not compile (#3973). Such a name is emitted with an `rb_` infix now.
def str_hash
  1
end
def poly_add(a, b)
  a + b
end
def gc_stat = "mine"
def box_it(x) = [x]
def my_helper(v) = v * 2
def plain = 7
p str_hash
p poly_add(1, 2)
p gc_stat
p box_it(3)
p my_helper(4)
p plain
alias str_hash2 str_hash
p str_hash2
module M
  def self.str_hash = 9
end
p M.str_hash
