# A method reached through a run-time alias answers the alias from
# `__callee__` and its own name from `__method__`, also from a block inside
# it and through an alias of an alias. A method it calls keeps its own name,
# its frame keeps the source's label, and an alias and its source are the
# same Method to `==`.
class R
  def base = [__method__, __callee__]
  def in_block = [1].map { __callee__ }.first
  def outer = [__callee__, inner]
  def inner = __callee__
  def boom = raise("x")
end
R.send(:alias_method, :late, :base)
R.send(:alias_method, :later, :late)
R.send(:alias_method, :blk_late, :in_block)
R.send(:alias_method, :outer_late, :outer)
R.send(:alias_method, :boom_late, :boom)
r = R.new
p r.base, r.late, r.later
p [r.in_block, r.blk_late]
p [r.outer, r.outer_late]
p [r.method(:late).name, r.method(:late).original_name, r.method(:later).original_name]
p r.method(:late) == r.method(:base)
p r.method(:late).owner
begin
  r.boom_late
rescue => e
  p e.backtrace.first
end
String.send(:alias_method, :shout, :upcase)
p "a".shout
p r.base
__END__
[:base, :base]
[:base, :late]
[:base, :later]
[:in_block, :blk_late]
[[:outer, :inner], [:outer_late, :inner]]
[:late, :base, :base]
true
R
"lang/methods/a_runtime_alias_answers_its_own_callee.rb:11:in 'R#boom'"
"A"
[:base, :base]
