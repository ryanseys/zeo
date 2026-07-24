# A mutable String (String.new + <<) owns its internal buffer, freed by
# the object's finalizer. Boxing/coercing/returning that buffer as an
# immutable string must copy it, not alias it: otherwise a GC that
# collects the unreferenced builder frees the buffer out from under the
# still-referenced string (a use-after-free). Each path below captures
# the converted value, then churns the heap and forces a GC so a freed
# buffer would be reused before it is read back.

def build_io(n)
  io = String.new
  i = 0
  while i < n
    io << i.to_s
    io << ","
    i += 1
  end
  io
end

def hold(x)                 # poly param: arg is boxed via sp_box_str(->data)
  GC.start
  i = 0
  t = ""
  while i < 4000
    t = t + i.to_s
    i += 1
  end
  x
end

def via_to_s(n)             # mutable.to_s returned across a fn boundary
  build_io(n).to_s
end

def via_gsub(n)             # gsub with no match returns the receiver
  build_io(n).gsub("ZZZ", "x")
end

hold(42)                    # int call site forces hold's param to poly

a = hold(build_io(6))       # poly-box conversion
b = via_to_s(6)             # to_s conversion
c = String(build_io(6))     # Kernel#String conversion
d = via_gsub(6)             # no-op gsub returning the receiver

i = 0
t = ""
while i < 8000
  t = t + i.to_s
  i += 1
end
GC.start
i = 0
while i < 8000
  t = t + i.to_s
  i += 1
end

puts a
puts b
puts c
puts d
