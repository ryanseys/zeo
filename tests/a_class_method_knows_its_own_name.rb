# `__method__` answers the BARE method name, whichever way the method was
# defined.
#
# zeo read it off the backtrace label and stripped only the `#` separator, so
# an instance method answered `:name` and a class method answered `:"Owner.name"`.
#
# What that breaks is `to_enum(__method__, ...)` -- the standard way a method
# hands back an enumerator over itself. The enumerator named a method that does
# not exist, so re-entering it raised NoMethodError. rubygems' vendored
# `Gem::TSort.tsort` is written exactly that way, so no gem could be resolved.

module M
  def self.plain = __method__
  def dup_name = __method__
  def self.dup_name = __method__
end

class C
  def self.klass = __method__
  def inst = __method__
  class << self
    def in_sclass = __method__
  end
end

def top_level = __method__

puts "module singleton\t#{M.plain.inspect}"
puts "module both, class\t#{M.dup_name.inspect}"
puts "class singleton\t#{C.klass.inspect}"
puts "instance\t#{C.new.inst.inspect}"
puts "class << self\t#{C.in_sclass.inspect}"
puts "top level\t#{top_level.inspect}"
puts "through Method\t#{M.method(:plain).call.inspect}"

# The idiom that made it matter.
module Walker
  def self.each_pair(a, b)
    return to_enum(__method__, a, b) unless block_given?

    yield a
    yield b
  end
end

puts "to_enum\t#{Walker.each_pair(1, 2).to_a.inspect}"
puts "with block\t#{Walker.each_pair(3, 4) { |x| }.inspect}"

# A method with a same-named instance twin still names itself, not its twin.
module Twin
  def go = "instance"

  def self.go
    return to_enum(__method__) unless block_given?

    yield :singleton
  end
end
puts "twin\t#{Twin.go.to_a.inspect}"

# `__callee__` answers the name it was REACHED through. The ALIAS half of that
# is a separate divergence and lives in
# tests/gaps/a_singleton_alias_reports_the_name_it_was_called_by.rb.
class Aliased
  def self.original = __callee__
end
puts "callee original\t#{Aliased.original.inspect}"
