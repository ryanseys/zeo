# `rescue => target` accepts every lvalue Ruby accepts -- including an
# attribute setter and an index write, which run only when the clause
# fires (receiver evaluated at that moment, then `attr=`/`[]=` called
# with the exception). zeo lowered only the variable-shaped targets and
# rejected these two.
class Holder
  attr_accessor :err
end

h = Holder.new
begin
  raise ArgumentError, "boom"
rescue => h.err
  puts "rescued into attr"
end
p h.err.class
p h.err.message

slot = {}
begin
  raise KeyError, "missing"
rescue KeyError => slot[:why]
  puts "rescued into index"
end
p slot[:why].class

# The receiver expression is evaluated when the clause fires, not before.
def fresh
  $built += 1
  Holder.new
end
$built = 0
begin
  raise "later"
rescue => fresh.err
end
p $built
__END__
rescued into attr
ArgumentError
"boom"
rescued into index
KeyError
1
