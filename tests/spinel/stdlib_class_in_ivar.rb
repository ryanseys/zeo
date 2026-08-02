# A `.new` on a stdlib class Spinel does not implement (Pathname, OpenStruct,
# IPAddr, ...) used to silently degrade to an inert 0 -- the object's methods
# then returned nil, so a program that actually used it diverged from CRuby with
# no signal. It now raises NameError instead: if it can't work, it fails loudly.
#
# A class Spinel models as a no-op (Mutex, single-threaded) and user-defined
# classes are unaffected. The ivar slot still compiles either way (the raise
# expression is int-typed, so no undeclared `sp_<Class> *` field is emitted).
#
# ostruct/ipaddr are unimplemented stdlib: under SPINEL_REQUIRE_GATE the
# `require` itself is a compile error, so a real program omits it and the .new
# still raises NameError at the use site. Mutex is core (no require needed).
# `Pathname` USED to be in that list; ruby 4.0 loads it before the first line,
# so it constructs here exactly as it does there -- the fixture said `pathname
# raised` and ruby says `no raise`.

class WithMutex
  def initialize
    @lock = Mutex.new   # special-cased no-op; works
    @n = 5
  end
  def n
    @n
  end
end

class WithPathname
  def initialize
    @path = Pathname.new('/tmp')   # unsupported stdlib class -> raises here
    @v = 7
  end
  def v
    @v
  end
end

# A Mutex ivar works (single-threaded no-op); another ivar reads fine.
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

# `Pathname` is core, so this one constructs.
begin
  WithPathname.new
  puts "no raise"
rescue NameError
  puts "pathname raised"
end

# A direct `.new` on an unsupported stdlib class raises too.
begin
  OpenStruct.new
  puts "no raise"
rescue NameError
  puts "openstruct raised"
end

puts "done"
