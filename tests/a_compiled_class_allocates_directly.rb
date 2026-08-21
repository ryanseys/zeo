# `Foo.new` on a plain compiled class allocates without dispatching for
# `new` at all: the runtime's `new` is served by the CONSTRUCTOR, not by a
# class-method row, so the call site's cache could only ever remember the
# miss and re-walk the singleton chain per allocation.
#
# The gate is what this file is really about -- every shape below has to
# keep the dynamic route, because each is a question only the run time can
# answer:
#   - an own `def self.new` overrides the constructor
#   - `private_class_method :new` raises NoMethodError from the walk
#   - a class with no `initialize` rejects arguments as `Object#initialize`
#   - a raise from `initialize` is attributed to `initialize`, not the site
class A
  def initialize(x = 5, *rest, &b) = (@x = x; @r = rest; @b = b ? b.call : nil)
  def to_s = "A(#{@x},#{@r},#{@b})"
end
class B < A; end
class Priv
  def self.make = new
  private_class_method :new
end
class Own
  def self.new(*a) = "own new #{a.inspect}"
end
class NoInit; end
puts A.new
puts A.new(1, 2, 3)
puts A.new { 42 }
puts B.new(9)
p Priv.make.class
begin; Priv.new; rescue NoMethodError => e; puts e.message; end
p Own.new(1)
p NoInit.new.class
begin; NoInit.new(1); rescue ArgumentError => e; puts e.message; end
class Raiser; def initialize = raise("boom"); end
begin; Raiser.new; rescue => e; puts "#{e.message} @ #{e.backtrace.first}"; end
