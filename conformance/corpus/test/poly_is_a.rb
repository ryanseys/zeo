# `<poly>.is_a?(Klass)` / `kind_of?` / `instance_of?` runtime
# dispatch. Spinel previously fell through to user-class dispatch
# only — the SP_TAG_INT / SP_TAG_STR / SP_TAG_FLT / SP_TAG_NIL /
# SP_TAG_BOOL / SP_TAG_SYM / built-in Array / Hash / Range cases
# were missing, so `is_a?(Integer)` against a poly slot always
# returned false and downstream branches took the wrong arm. The
# user-class dispatch also walked the wrong direction (recv->parent
# instead of any-descendant->ancestor).
#
# Repro: a heterogeneous poly array yields each element back as
# poly, then the type test routes it to the right branch. The
# trailing Bar.new must select the `is_a?(Bar)` arm AND the
# Foo.new before it must NOT match `is_a?(Bar)` (subclass-only).

class Foo
  def name; "foo"; end
end
class Bar < Foo
  def name; "bar"; end
end

arr = [42, "hello", :sym, 1.5, nil, true, false, [1, 2], Foo.new, Bar.new]
arr.each do |x|
  if x.is_a?(Integer)
    puts "Integer #{x}"
  elsif x.is_a?(String)
    puts "String #{x}"
  elsif x.is_a?(Symbol)
    puts "Symbol #{x.to_s}"
  elsif x.is_a?(Float)
    puts "Float"
  elsif x.is_a?(NilClass)
    puts "Nil"
  elsif x.is_a?(TrueClass)
    puts "True"
  elsif x.is_a?(FalseClass)
    puts "False"
  elsif x.is_a?(Array)
    puts "Array"
  elsif x.is_a?(Bar)
    puts "Bar"
  elsif x.is_a?(Foo)
    puts "Foo"
  else
    puts "?"
  end
end
