# A definition hook sees the class only as far as it has been built. The
# REFLECTION half of that is `tests/method_added_watermark.rb`; this is the
# DISPATCH half -- calling a method written below the `def` the hook is
# reporting must miss, exactly as if it were not there yet.
#
# The two are different machines. Reflection asks `not_yet_defined` on its way
# to an answer it was already computing. Dispatch resolves through an inline
# cache and the flattened method tables, so the same question has to be asked
# ahead of the cache -- one relaxed atomic load, and the whole call takes the
# slow route only while a hook is actually running.

class Base
  def shared = "from Base"
end

class Probe < Base
  def self.method_added(name)
    return unless name == :first
    [-> { new.later },
     -> { new.send(:later) },
     -> { new.public_send(:later) },
     -> { instance_method(:later) },
     -> { new.method(:later) },
     -> { new.respond_to?(:later) },
     -> { method_defined?(:later) },
     -> { instance_methods(false).sort },
     # An INHERITED name is untouched: ruby hides only what exists nowhere
     # yet, so `Base#shared` still answers though `Probe#shared` is below.
     -> { new.shared },
     # ...and so does anything the class never redefines at all.
     -> { new.to_s.class }].each do |f|
      begin
        puts "  ok: #{f.call.inspect}"
      rescue => e
        puts "  #{e.class}: #{e.message}"
      end
    end
  end

  def first = 1
  def later = 2
  def shared = "from Probe"
end

puts "== and every one of them answers once the class is whole"
p [Probe.new.later, Probe.new.shared, Probe.instance_methods(false).sort]

puts "== method_missing still gets its chance"
class WithMM
  def self.method_added(name)
    return unless name == :one
    puts "  #{new.two}"
  end

  def method_missing(name, *args) = "method_missing(#{name})"
  def respond_to_missing?(name, priv = false) = true

  def one = 1
  def two = 2
end
p WithMM.new.two

puts "== the truncation lifts as soon as the hook returns"
class Sequential
  def self.method_added(name)
    puts "  added #{name}, b reachable: #{new.respond_to?(:b)}"
  end

  def a = 1
  def b = 2
end
p [Sequential.new.a, Sequential.new.b]

puts "== an unrelated class is never truncated"
class Other
  def wide = "other"
end

class Narrow
  def self.method_added(name)
    return unless name == :x
    puts "  Other#wide -> #{Other.new.wide}"
  end

  def x = 1
  def wide = "narrow"
end
p [Narrow.new.wide, Other.new.wide]
