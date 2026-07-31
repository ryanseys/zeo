class Base
  @reg = "base-ivar"
  @@cv = "base-cvar"
  def self.reg = @reg
  def self.cv = @@cv
  def self.bump; @n = (@n || 0) + 1; end
  def self.never = @never_written
  def self.pair = [@reg, @reg]
end
class Sub < Base; end

p Base.reg
p Sub.reg
p Sub.cv
p Base.pair

# Reading a never-assigned class ivar answers nil and must NOT make the name
# appear in `instance_variables` -- the read is what first reserves storage.
p Base.never
p Base.instance_variables

Base.bump
Base.bump
p Base.instance_variable_get(:@n)
p Base.instance_variables.sort

# A name the compiler never saw still reaches the same storage.
Base.instance_variable_set(:@invented, 7)
p Base.instance_variable_get(:@invented)
p Base.instance_variables.sort
p Base.instance_variable_defined?(:@never_written)
p Base.instance_variable_defined?(:@reg)

# A subclass gets its own slot, written through the inherited class method.
Sub.bump
p [Base.instance_variable_get(:@n), Sub.instance_variable_get(:@n)]

# One class-level ivar bumped in a thread is visible to the parent.
module Counter
  @hits = 0
  def self.hit; @hits = @hits + 1; end
  def self.hits = @hits
end
Thread.new { 5.times { Counter.hit } }.join
p Counter.hits

Base.freeze
begin
  Base.bump
rescue => e
  p [e.class, e.message]
end
