# rescue of a namespaced exception class (rescue M::Err) must match a
# raised M::Err. Both the raise and the rescue clause need to resolve
# the ConstantPathNode (M::Err) to the registered class name (M_Err);
# previously raise emitted a bare sp_raise (no class) and rescue
# checked only the leaf name, so the handler never matched.
module M
  class Err < StandardError; end
end

begin
  raise M::Err, "boom"
rescue M::Err => e
  puts "caught: " + e.message
end

# Control: bare base class still matches a namespaced raise.
begin
  raise M::Err, "again"
rescue StandardError => e
  puts "base: " + e.message
end

# Control: top-level class name still matches.
class TopErr < StandardError; end
begin
  raise TopErr, "top"
rescue TopErr => e
  puts "top: " + e.message
end

puts "after"
