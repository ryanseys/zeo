# One emitted body serves a parent and every descendant, and it indexes
# `@x` by the slot the PARENT gave it. That is only sound because
# `analyze::mro` lays each class out as `ivars(parent) ++ its own new
# names`, so a descendant agrees about every slot its parent declared.
#
# The classes below disagree about everything else they can: each adds its
# own ivars, in different numbers, some before their first use of an
# inherited body and some after, and two of them add a module that brings
# more. A shared body reading one slot too far answers another ivar's
# value rather than raising, so the assertions read the values back.

module Extra
  def extra_set; @e1 = "e1"; @e2 = "e2"; nil; end
end

class Parent
  def write(v); @x = "<#{v}>"; nil; end
  def read; @x.nil? ? "unset" : @x.upcase; end
end

class Bare < Parent; end

class Adds < Parent
  def own_set; @a1 = 1; @a2 = 2; nil; end
end

class Mixes < Parent
  include Extra
  def own_set; @m1 = "m"; nil; end
end

class Deep < Adds
  include Extra
  def deep_set; @d1 = :d; nil; end
end

[Parent, Bare, Adds, Mixes, Deep].each do |k|
  o = k.new
  # Fill the subclass's OWN ivars first where it has them, so a body
  # reading the wrong slot finds a plausible value rather than nil.
  o.own_set if o.respond_to?(:own_set)
  o.extra_set if o.respond_to?(:extra_set)
  o.deep_set if o.respond_to?(:deep_set)
  o.write(k.name)
  puts "#{k}\t#{o.read}\t#{o.instance_variables.inspect}"
end

# The reverse direction: write through the shared body, then read the ivar
# by name. The two must agree, whatever slot the body chose.
[Parent, Bare, Adds, Mixes, Deep].each do |k|
  o = k.new
  o.write("direct")
  p [k.to_s, o.instance_variable_get(:@x), o.read]
end
__END__
Parent	<PARENT>	[:@x]
Bare	<BARE>	[:@x]
Adds	<ADDS>	[:@a1, :@a2, :@x]
Mixes	<MIXES>	[:@m1, :@e1, :@e2, :@x]
Deep	<DEEP>	[:@a1, :@a2, :@e1, :@e2, :@d1, :@x]
["Parent", "<direct>", "<DIRECT>"]
["Bare", "<direct>", "<DIRECT>"]
["Adds", "<direct>", "<DIRECT>"]
["Mixes", "<direct>", "<DIRECT>"]
["Deep", "<direct>", "<DIRECT>"]
