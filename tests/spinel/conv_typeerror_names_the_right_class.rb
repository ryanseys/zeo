# The class a conversion TypeError names has to survive the emission that sits
# between taking it and writing it. class_ruby_name built the qualified name
# into a shared static buffer, so a message built as
#
#   cn = class_ruby_name(...);  emit_expr(arg);  printf("...%s...", cn)
#
# printed whichever class the argument's own emission asked about. Below, the
# inner conversion is EMITTED but never runs (its branch is not taken), and it
# was enough to rename the error the outer one raises.
#
# The same buffer is why `is_a?` on a namespaced exception compared its
# receiver against itself (#4133).
module M
  class Alpha; end
  class Beta; end
end

x = M::Beta.new
cond = (ARGV.length > 99)

begin
  p 1.0.quo(cond ? ((1.0.quo(M::Alpha.new) > 0.0) ? x : x) : x)
rescue TypeError => e
  p e.message
end

# The same shape through the Array and Hash protocols, which name a class the
# same way.
begin
  p [1, 2].product(cond ? ((1.0.quo(M::Alpha.new) > 0.0) ? x : x) : x)
rescue TypeError => e
  p e.message
end

begin
  p({ 1 => 2 }.merge(cond ? ((1.0.quo(M::Alpha.new) > 0.0) ? x : x) : x))
rescue TypeError => e
  p e.message
end

# A top-level class is not qualified and never used the buffer, so it was
# right all along -- pinned so a future change cannot take it the other way.
class Plain; end
begin
  p 1.0.quo(Plain.new)
rescue TypeError => e
  p e.message
end

# Two nested classes named in one expression, each keeping its own name.
begin
  p 1.0.quo(M::Alpha.new)
rescue TypeError => e
  p e.message
end
begin
  p 1.0.quo(M::Beta.new)
rescue TypeError => e
  p e.message
end
