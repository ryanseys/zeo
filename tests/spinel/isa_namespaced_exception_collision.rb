# `is_a?` against a namespaced exception class answered false when another
# module held a class of the same leaf name. Two things had to line up:
#
#  - the colliding class is carried in the AST already flattened (`A::Error`
#    arrives as `A__Error`), and the is_a? site prefixed its module again,
#    asking about "A::A__Error" -- a name nothing answers to;
#  - the repair for that read the class table's qualified name, which comes
#    back in a SHARED static buffer that emitting the receiver then
#    overwrote, so every check compared the receiver against itself.
#
# `rescue` and #ancestors were right throughout, which is what made it quiet:
# only is_a?/kind_of?/instance_of? saw it. Found under net/http, where
# Timeout::Error sits beside URI::Error. (matz/spinel#4133)
module A
  class Error < StandardError; end
  class Sub < Error; end
end

module B
  class Error < RuntimeError; end
  class Sub < Error; end
end

a = A::Sub.new("x")
b = B::Sub.new("x")

# Each answers its own namespace's parent...
p a.is_a?(A::Error)
p b.is_a?(B::Error)
# ...and not the other one's, which shares the leaf name.
p a.is_a?(B::Error)
p b.is_a?(A::Error)

# The builtin grandparents still answer, and they differ between the two.
p a.is_a?(StandardError)
p b.is_a?(RuntimeError)
p a.is_a?(RuntimeError)

# instance_of? goes through the same name, and is exact.
p a.instance_of?(A::Sub)
p a.instance_of?(A::Error)

# ancestors was never wrong; pinned so the two cannot drift apart.
p A::Sub.ancestors.take(3).map(&:to_s)
p B::Sub.ancestors.take(3).map(&:to_s)

# rescue was never wrong either.
begin
  raise A::Sub, "boom"
rescue B::Error
  puts "WRONG namespace"
rescue A::Error => e
  puts "rescued: #{e.class}"
end
