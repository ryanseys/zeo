# From the spinel corpus (c55d9bdb).
# initialize, initialize_copy and friends are not private by default.
#
class Hooked
  attr_accessor :v
  def initialize(v); @v = v; end
  def initialize_copy(src); @v = src.v; end
end
p Hooked.private_method_defined?(:initialize_copy)

p Hooked.public_method_defined?(:initialize_copy)     # Ruby: false   Spinel: true
class Plain; def initialize(v); @v = v; end; end
p Plain.private_method_defined?(:initialize_copy)     # Ruby: true    Spinel: false

class Plain2
  attr_accessor :v
  def initialize(v); @v = v; end
end
o001 = Plain2.new(1); o001.send(:initialize_copy, Plain2.new(5)); p o001.v   # => 1
p Plain2.new(1).respond_to?(:initialize_copy, true)                          # => true
p Hooked.new(1).respond_to?(:initialize_copy, true)                          # => true
__END__
true
false
true
1
true
true
