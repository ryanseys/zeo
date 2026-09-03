# A String's `I`-wrapper carries its encoding -- `E => true` (UTF-8),
# `E => false` (US-ASCII), `encoding => "<name>"` (other), and NO wrapper
# for ASCII-8BIT. The encoding survives a round-trip.

def wire(x) = Marshal.dump(x).bytes.join(",")
def rt(x) = Marshal.load(Marshal.dump(x))
puts wire("abc")
puts wire("abc".b)
puts wire("abc".encode("US-ASCII"))
puts wire("café")
puts wire("abc".encode("Shift_JIS"))
puts rt("café").encoding.name
puts rt("abc".b).encoding.name
puts rt("abc".encode("Shift_JIS")).encoding.name
puts rt("hello")
__END__
4,8,73,34,8,97,98,99,6,58,6,69,84
4,8,34,8,97,98,99
4,8,73,34,8,97,98,99,6,58,6,69,70
4,8,73,34,10,99,97,102,195,169,6,58,6,69,84
4,8,73,34,8,97,98,99,6,58,13,101,110,99,111,100,105,110,103,34,14,83,104,105,102,116,95,74,73,83
UTF-8
ASCII-8BIT
Shift_JIS
hello
