# The frozen-class guard family: civar/cvar writes (static paths and
# reflection), runtime define_method / define_singleton_method, plus
# frozen-object ivar/singleton/extend guards and the frozen-builtin
# instance_variable_set raises. The cvar guard is on the storage OWNER
# (`Sub.freeze` doesn't stop a write to Base's `@@z`). Expected output
# is verbatim ruby 4.0.6 (addresses normalized in-script).

class Foo
  def self.setiv; @x = 1; end
  def self.setcv; @@y += 1; end
  def bump; @@y = 9; end
  @@y = 0
end
module Bar; end
puts Foo.frozen?
Foo.freeze
puts Foo.frozen?
Bar.freeze
puts Bar.frozen?
def try
  yield
rescue => e
  puts "#{e.class}: #{e.message.sub(/0x[0-9a-f]+/, "0xADDR")}"
end
try { Foo.instance_variable_set(:@a, 1) }
try { Foo.class_variable_set(:@@b, 1) }
try { Foo.setiv }
try { Foo.setcv }
try { Foo.new.bump }
name = :c
try { Foo.define_singleton_method(name) { 1 } }
try { Foo.send(:define_method, :m2) { 1 } }
class Base
  @@z = 1
  def self.setz; @@z = 5; end
  def self.z; @@z; end
end
class Sub < Base; end
Sub.freeze
Sub.setz
puts Base.z
o = Object.new.freeze
try { o.instance_variable_set(:@v, 1) }
try { o.define_singleton_method(name) { 2 } }
try { o.extend(Comparable) }
s = "st".freeze
try { s.instance_variable_set(:@v, 1) }
try { 5.instance_variable_set(:@v, 1) }
__END__
false
true
true
FrozenError: can't modify frozen Class: Foo
FrozenError: can't modify frozen Class: Foo
FrozenError: can't modify frozen Class: Foo
FrozenError: can't modify frozen Class: Foo
FrozenError: can't modify frozen Class: Foo
FrozenError: can't modify frozen Class: Foo
FrozenError: can't modify frozen Class: Foo
5
FrozenError: can't modify frozen Object: #<Object:0xADDR>
FrozenError: can't modify frozen Object: #<Object:0xADDR>
FrozenError: can't modify frozen Object: #<Object:0xADDR>
FrozenError: can't modify frozen String: "st"
FrozenError: can't modify frozen Integer: 5
