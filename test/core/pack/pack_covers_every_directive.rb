# `p` and `P` write a MACHINE POINTER, so the packed string has to keep the
# strings it points at reachable: CRuby hangs them off the result in a hidden
# ivar and `unpack` looks the pointer up there, handing back THE ORIGINAL
# STRING OBJECT -- which is why `p`'s answer keeps that string's encoding and
# is `equal?` to it. A pointer no list explains is an error, but a NULL one is
# just nil, so eight zero bytes unpack fine.
#
# `P<n>`'s count is a WIDTH, not a repeat: it checks the buffer is at least
# that long, then writes one pointer and consumes one element.

def t(label)
  r = begin
    yield.inspect
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts format("%-14s %s", label, r)
end

t("p size") { ["x"].pack("p").bytesize }
t("p enc") { ["x"].pack("p").encoding.to_s }
t("p ivars") { ["x"].pack("p").instance_variables }
t("p rt") { ["hello"].pack("p").unpack1("p") }
t("p rt enc") { ["hello"].pack("p").unpack1("p").encoding.to_s }
t("p identity") { a = "who"; [a].pack("p").unpack1("p").equal?(a) }
t("p2 size") { ["a", "b"].pack("p2").bytesize }
t("p2 rt") { ["a", "b"].pack("p2").unpack("p2") }
t("p star") { ["a", "b"].pack("p*").unpack("p*") }
t("p nil") { [nil].pack("p").bytes }
t("p nilptr") { [nil].pack("p").unpack1("p") }
t("p nonstr") { [1].pack("p") }
t("p frozen") { ["x"].pack("p").frozen? }
t("p dup rt") { s = ["hey"].pack("p"); s.dup.unpack1("p") }
t("p unassoc") { ("\x00" * 8).unpack1("p") }
t("p junk") { ("\x01" * 8).unpack1("p") }

t("P5 size") { ["hello"].pack("P5").bytesize }
t("P5 rt") { ["hello"].pack("P5").unpack1("P5") }
t("P no len") { ["hello"].pack("P").unpack1("P") }
t("P big len") { ["hi"].pack("P").unpack1("P9") }
t("P short") { ["hi"].pack("P9") }
t("P nil") { [nil].pack("P").bytes }
t("P unassoc") { ("\x00" * 8).unpack1("P5") }

t("mixed") { ["ab", 7].pack("pN").unpack("pN") }
__END__
p size         8
p enc          "ASCII-8BIT"
p ivars        []
p rt           "hello"
p rt enc       "UTF-8"
p identity     true
p2 size        16
p2 rt          ["a", "b"]
p star         ["a", "b"]
p nil          [0, 0, 0, 0, 0, 0, 0, 0]
p nilptr       nil
p nonstr       TypeError: no implicit conversion of Integer into String
p frozen       false
p dup rt       "hey"
p unassoc      nil
p junk         ArgumentError: no associated pointer
P5 size        8
P5 rt          "hello"
P no len       "h"
P big len      "hi"
P short        ArgumentError: too short buffer for P(2 for 9)
P nil          [0, 0, 0, 0, 0, 0, 0, 0]
P unassoc      nil
mixed          ["ab", 7]
