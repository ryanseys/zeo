# catch/throw delivers a value across method boundaries; a throw whose tag has
# no live catch raises UncaughtThrowError (rescuable), rather than leaking.

def thrower; throw :done, [1, 2]; end
def catcher; catch(:done) { thrower } end
p catcher                              # [1, 2]

# object tag matched by identity; inner throw can target an outer catch
outer = Object.new
inner = Object.new
r = catch(outer) do
  catch(inner) { throw outer, :to_outer }
  :not_reached
end
p r                                    # :to_outer

# no matching catch -> UncaughtThrowError, caught by an ordinary rescue
begin
  throw :nope, 5
  puts "WRONG"
rescue UncaughtThrowError => e
  puts e.message                       # uncaught throw :nope
end

# a throw to a catch that has already exited is also uncaught
tag = :gone
catch(tag) { }
begin
  throw tag
rescue UncaughtThrowError => e
  puts "gone: #{e.message}"
end

# W3e: array pattern with a trailing comma is an implicit (anonymous) rest
case [0, 1, 2, 3]
in [0, 1, ] then puts "at-least matched"
else puts "no"
end
case [5]
in [0, 1, ] then puts "wrong"
else puts "too short"
end

# W3f: splat into a Proc/lambda receiver
add3 = ->(a, b, c) { a + b + c }
args = [1, 2, 3]
p add3.call(*args)                     # 6
p add3[*args]                          # 6
p add3.(*args)                         # 6
prc = proc { |a, b| a * b }
p prc.call(*[4, 5])                    # 20
__END__
[1, 2]
:to_outer
uncaught throw :nope
gone: uncaught throw :gone
at-least matched
too short
6
6
6
20
