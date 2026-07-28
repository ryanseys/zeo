# `undef :m if <runtime condition>` -- drb's shape. The guard can't be decided
# at compile time, so the undef has to become a runtime `undef_method` rather
# than the compile-time record an unguarded `undef` is.
class Ghost
  def to_a = [1]
  def to_s = "ghost"
  def size = 0

  # `self` in a class body is the CLASS, so this asks whether Ghost-the-object
  # responds -- it doesn't, and the undef is skipped.
  undef :to_a if respond_to?(:to_a, true)
  undef :to_s unless $stdout.nil?
  undef :size, :nonexistent_is_fine if false
end

p Ghost.new.respond_to?(:to_a)
p Ghost.new.respond_to?(:to_s)
p Ghost.new.respond_to?(:size)
p Ghost.instance_methods(false).sort

# An undef'd name stays gone through every reflection door, and `super` can't
# reach past it either.
class Base
  def greet = "base"
end
class Quiet < Base
  undef :greet if [1].size == 1
end
p Quiet.method_defined?(:greet)
p Quiet.new.respond_to?(:greet)
begin
  Quiet.new.greet
rescue NoMethodError => e
  puts e.message
end
