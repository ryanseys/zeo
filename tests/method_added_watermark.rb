# Ruby's definition hook sees a HALF-BUILT class: at `method_added(:a)`,
# `instance_methods(false)` answers `[:a]` alone, and a `def b` written below is
# not defined yet.
#
# zeo installs every method table before the program's first statement runs, so
# there is no such moment to observe. The compiler works out which names are
# still in the future at each announcement -- it knows them statically -- and
# the reflection rows subtract that set for as long as the hook body is running.

class W
  def self.method_added(name)
    puts "#{name}: own=#{instance_methods(false).sort.inspect} " \
         "defined?(later)=#{method_defined?(:later)} " \
         "respond_to=#{new.respond_to?(:later)}"
  end

  def first = 1
  def later = 2
  def last = 3
end

puts "-- once the class is built, everything is visible again"
p [W.instance_methods(false).sort, W.method_defined?(:later)]

puts "-- instance_method raises for a name that does not exist yet"
class R
  def self.method_added(name)
    return unless name == :one
    begin
      instance_method(:two)
    rescue NameError => e
      puts "  NameError: #{e.message}"
    end
  end
  def one = 1
  def two = 2
end

puts "-- visibility predicates follow the same watermark"
class V
  def self.method_added(name)
    return unless name == :pub
    puts "  public?=#{public_method_defined?(:priv)} private?=#{private_method_defined?(:priv)}"
  end
  def pub = 1
  private
  def priv = 2
end

puts "-- an INHERITED name is not hidden: it exists, whatever this class reached"
class Base
  def shared = "base"
end
class Sub < Base
  def self.method_added(name)
    return unless name == :own
    puts "  shared defined?=#{method_defined?(:shared)} own=#{instance_methods(false).sort.inspect}"
  end
  def own = 1
  def shared = "sub"
end

puts "-- a redefinition does not hide the name it already defined"
class D
  def self.method_added(name)
    puts "  #{name}: defined?=#{method_defined?(:x)}"
  end
  def x = 1
  def x = 2
end

puts "-- a runtime definition needs no watermark: it really is the last one"
K = Class.new do
  def self.method_added(name)
    puts "  runtime #{name}: own=#{instance_methods(false).sort.inspect}"
  end
  def a = 1
  def b = 2
end
