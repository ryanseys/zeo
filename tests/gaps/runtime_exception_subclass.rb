# `Class.new(StandardError)` builds a class whose ancestry is right but whose
# allocator does not produce an exception: `.new("msg")` panics instead of
# answering an instance, so a runtime-minted error class can be defined but
# never raised. Every exception zeo can construct is one it compiled a
# `class X < StandardError` for.
R = Class.new(StandardError)
p R.ancestors.first(3)
e = R.new("hi")
p e.class
p e.message
p e.is_a?(StandardError)
begin
  raise R, "boom"
rescue R => ex
  p [:rescued, ex.message]
end
