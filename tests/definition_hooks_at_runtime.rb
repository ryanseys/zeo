# Ruby reports every definition to the class it landed on. This file covers the
# definitions made AT RUNTIME -- `define_method`, `class_eval`, `Class.new`, a
# singleton def -- where the hook fires from the runtime writer itself.
#
# One rule drives all six names (CRuby's `CALL_METHOD_HOOK`): a definition on a
# singleton reports to the attached OBJECT under the `singleton_` name;
# everything else reports to the class.

Base = Class.new do
  def self.method_added(n) = puts("added #{n}")
  def self.method_removed(n) = puts("removed #{n}")
  def self.method_undefined(n) = puts("undefined #{n}")
  # Installed last, so it is the only one of the four that reports itself.
  def self.singleton_method_added(n) = puts("s_added #{n}")
end

puts "-- define_method"
Base.send(:define_method, :a) { 1 }
puts "-- attr_accessor reports the reader, then the writer"
Base.class_eval { attr_accessor :c }
puts "-- alias_method reports the NEW name"
Base.send(:alias_method, :d, :a)
puts "-- define_singleton_method"
Base.define_singleton_method(:sing) { 2 }
puts "-- define_method from a Method object"
Base.send(:define_method, :e, Base.instance_method(:a))
puts "-- remove_method"
Base.send(:remove_method, :d)
puts "-- undef_method"
Base.send(:undef_method, :a)

# `private :m` on the class's OWN method sets the visibility in place and
# reports nothing. On an INHERITED one ruby synthesizes a fresh entry, which
# IS a definition.
puts "-- private, own vs inherited"
Base.send(:define_method, :own) { }
Base.send(:private, :own)
Base.send(:private, :inspect)

# A per-object singleton reports to that object alone.
puts "-- per-object singleton"
o = Object.new
o.define_singleton_method(:singleton_method_added) { |n| puts "o s_added #{n}" }
o.define_singleton_method(:bar) { }
def o.written_as_a_def = 1
class << o
  def written_in_a_singleton_class = 2
end

# `module_function` makes an instance copy AND a module method, and ruby
# reports both -- but the explicit-name form reports only the module half,
# because the instance copy already existed.
puts "-- module_function"
M = Module.new do
  def self.method_added(n) = puts("M added #{n}")
  def self.singleton_method_added(n) = puts("M s_added #{n}")
end
M.send(:define_method, :mm) { }
M.send(:module_function, :mm)

# A `Numeric` is meant to be interchangeable with any equal value, so ruby
# refuses it a singleton at all -- through this very hook.
puts "-- Numeric refuses a singleton"
class Deg < Numeric
  def initialize(v) = @v = v
  def inspect = "Deg"
end
n = Deg.new(7)
[-> { def n.frob = :frobbed }, -> { n.define_singleton_method(:frob) { } }].each do |attempt|
  attempt.call
rescue TypeError => e
  puts "numeric: #{e.message}"
end

# `include`/`extend` splice a module in; they define nothing, so nothing fires.
puts "-- include and extend report nothing"
Mixin = Module.new { def mixed = 1 }
Base.send(:include, Mixin)
o.extend(Mixin)
puts "done"
