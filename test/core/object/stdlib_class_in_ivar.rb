# An ivar holding a library object, and what happens when the library was
# never required.
#
# A required library constructs and stores normally: `require "ipaddr"` then
# `IPAddr.new` in an initialize raises nothing, and the other ivars beside it
# read back. Mutex needs no require, and a user-defined class in an ivar is
# no different.
#
# A constant whose library was NOT required raises NameError at the use site,
# which is what plain Ruby does: OpenStruct is a name nothing has defined
# yet. The point is that it fails loudly rather than storing something inert
# whose methods then answer nil.

require "ipaddr"

class WithMutex
  def initialize
    @lock = Mutex.new
    @n = 5
  end
  def n
    @n
  end
end

class WithIPAddr
  def initialize
    @addr = IPAddr.new('127.0.0.1')
    @v = 7
  end
  def v
    @v
  end
end

# A Mutex ivar constructs, and the ivar beside it reads fine.
puts WithMutex.new.n          #=> 5

# A user-defined class is unaffected.
class Point
  def initialize(x)
    @x = x
  end
  def x
    @x
  end
end

class Holder
  def initialize
    @p = Point.new(42)
  end
  def px
    @p.x
  end
end

puts Holder.new.px            #=> 42

# ipaddr was required above, so the holder constructs.
begin
  WithIPAddr.new
  puts "no raise"
rescue NameError
  puts "ipaddr raised"
end

# A `.new` on a constant whose library was never required raises.
begin
  OpenStruct.new
  puts "no raise"
rescue NameError
  puts "openstruct raised"
end

puts "done"
__END__
5
42
no raise
openstruct raised
done
