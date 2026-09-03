# `Array#pack`'s bit-string and position directives, and the encoding the
# result carries. `unpack` had all of these; `pack` was missing `B`, `b` and
# `x` entirely and raised `unsupported pack directive`.

def show(label)
  puts format("%-22s %s", label, (yield).inspect)
rescue => e
  puts format("%-22s %s: %s", label, e.class, e.message.to_s[0, 44])
end

show("B bit string")      { ["01000001"].pack("B*").bytes }
show("b bit string")      { ["10000010"].pack("b*").bytes }
show("B partial byte")    { ["0100"].pack("B*").bytes }
show("b partial byte")    { ["1000"].pack("b*").bytes }
show("B fixed count")     { ["01000001"].pack("B4").bytes }
show("B non-binary char") { ["0x0y"].pack("B*").bytes }
show("B round trip")      { ["01000001"].pack("B*").unpack1("B*") }
show("b round trip")      { ["10000010"].pack("b*").unpack1("b*") }
show("x null pad")        { [].pack("x3").bytes }
show("x default")         { [].pack("x").bytes }
show("x between")         { [65, 66].pack("CxC").bytes }
show("X back up")         { [65, 66].pack("CCX").bytes }
show("@ absolute")        { [65].pack("C@4").bytes }

# The result's encoding: a template starts at US-ASCII and only moves down.
# `m`/`M`/`u` write ASCII text, `U` lifts to UTF-8, anything writing raw
# bytes pins it at ASCII-8BIT.
show("empty template")    { [].pack("").encoding.to_s }
show("B encoding")        { ["01"].pack("B*").encoding.to_s }
show("x encoding")        { [].pack("x").encoding.to_s }
show("m encoding")        { ["hi"].pack("m0").encoding.to_s }
show("M encoding")        { ["hi"].pack("M").encoding.to_s }
show("u encoding")        { ["hi"].pack("u").encoding.to_s }
show("U encoding")        { [233].pack("U*").encoding.to_s }
show("U then m")          { [65, "hi"].pack("Um").encoding.to_s }
show("m then U")          { ["hi", 65].pack("mU").encoding.to_s }
show("U then C")          { [65, 66].pack("UC").encoding.to_s }
show("C encoding")        { [65].pack("C*").encoding.to_s }
show("@ encoding")        { [].pack("@2").encoding.to_s }
__END__
B bit string           [65]
b bit string           [65]
B partial byte         [64]
b partial byte         [1]
B fixed count          [64]
B non-binary char      [16]
B round trip           "01000001"
b round trip           "10000010"
x null pad             [0, 0, 0]
x default              [0]
x between              [65, 0, 66]
X back up              [65]
@ absolute             [65, 0, 0, 0]
empty template         "US-ASCII"
B encoding             "ASCII-8BIT"
x encoding             "ASCII-8BIT"
m encoding             "US-ASCII"
M encoding             "US-ASCII"
u encoding             "US-ASCII"
U encoding             "UTF-8"
U then m               "UTF-8"
m then U               "UTF-8"
U then C               "ASCII-8BIT"
C encoding             "ASCII-8BIT"
@ encoding             "ASCII-8BIT"
