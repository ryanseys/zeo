# `private_constant` hides the SCOPE OPERATOR, not the reader: `M::S` raises
# everywhere -- even inside `M`'s own body -- while a bare `S` resolved through
# the cref still answers, and so does `M.const_get(:S)`. That split is what
# makes it a real access rule rather than a naming convention.

module M
  SECRET = 1
  OPEN = 2
  class Hidden; end
  module Nested; end
  private_constant :SECRET, :Hidden, :Nested

  def self.bare_value = SECRET
  def self.bare_class = Hidden
  def self.qualified = M::SECRET
end

puts "-- the listing omits them"
p M.constants.sort

puts "-- but the bindings are still there"
p M.const_defined?(:SECRET)
p M.const_defined?(:SECRET, false)
p M.const_get(:SECRET)
p M.const_get(:Hidden)
p Object.const_get("M::SECRET")

puts "-- a bare reference through the cref answers"
p M.bare_value
p M.bare_class

puts "-- the scope operator does not, from anywhere"
[
  -> { M::SECRET },
  -> { M::Hidden },
  -> { M::Nested },
  -> { M.qualified },
].each do |probe|
  begin
    probe.call
  rescue NameError => e
    puts e.message
  end
end
p M::OPEN

puts "-- defined? agrees with the reference, not with const_defined?"
p defined?(M::SECRET)
p defined?(M::Hidden)
p defined?(M::OPEN)
p defined?(M)

puts "-- public_constant restores it, at run time"
M.public_constant :SECRET
p M::SECRET
p defined?(M::SECRET)
p M.constants.sort

puts "-- and a subclass reads its parent's private constant bare"
class K
  A = 3
  private_constant :A
end
begin
  K::A
rescue NameError => e
  puts e.message
end
class Sub < K
  def self.peek = A
end
p Sub.peek

puts "-- const_get walks a path"
module Outer
  module Inner
    LEAF = 9
  end
end
p Object.const_get("Outer::Inner::LEAF")
p Object.const_get("Outer::Inner")
begin
  Object.const_get("Outer::Nope")
rescue NameError => e
  puts e.message
end
