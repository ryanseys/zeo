# `rescue A, *list => e` splats `list` by the SAME rule an argument list does.
#
# zeo tested the splat's value as if it were always one class, so `*nil` --
# which splices nothing -- was tested as a nil class and raised
# `TypeError: class or module required for rescue clause` INSTEAD of whatever
# the body raised. It only showed when no earlier entry matched, which is why
# it hid: the common path short-circuits before the splat is ever read.
#
# rubygems writes exactly that:
#
#     rescue Gem::Timeout::Error, IOError, SocketError, SystemCallError,
#            *(OpenSSL::SSL::SSLError if Gem::HAVE_OPENSSL) => e
#
# so every failed download reported a TypeError from a rescue clause instead
# of the error it was rescuing.

class Splattable
  def to_a = [ArgumentError, TypeError]
end

def catch_with(list)
  raise KeyError, "k"
rescue IOError, *list => e
  "caught #{e.class}"
end

[nil, [], [KeyError], KeyError, Splattable.new, false].each do |list|
  result = begin
    catch_with(list)
  rescue StandardError => e
    "#{e.class}: #{e.message}"
  end
  puts "#{list.class}\t#{result}"
end

# The splat entry really is consulted, not just tolerated.
def only_via_splat
  raise ArgumentError, "a"
rescue *[ArgumentError] => e
  "splat matched #{e.class}"
end
puts only_via_splat

# A guarded splat that evaluates to a class still matches through it.
HAVE = true
def guarded
  raise ArgumentError, "a"
rescue IOError, *(ArgumentError if HAVE) => e
  "guard matched #{e.class}"
end
puts guarded

# And the same clause with the guard false lets the exception past.
NONE = nil
def unguarded
  raise ArgumentError, "a"
rescue IOError, *(ArgumentError if NONE) => e
  "guard matched #{e.class}"
end
begin
  unguarded
rescue ArgumentError => e
  puts "guard absent, propagated #{e.class}"
end
__END__
NilClass	KeyError: k
Array	KeyError: k
Array	caught KeyError
Class	caught KeyError
Splattable	KeyError: k
FalseClass	TypeError: class or module required for rescue clause
splat matched ArgumentError
guard matched ArgumentError
guard absent, propagated ArgumentError
