# `encoding` on a String reached through a POLY receiver. The concrete String
# arm has answered it since #723 and the poly dispatch had no entry, so a
# String read out of a poly array raised NoMethodError naming its own class:
# "undefined method 'encoding' for an instance of String".
#
# Adding the arm was half of it. The call still had no inferred type, so
# emit_boxed's untyped arm rendered `(expr, sp_box_nil())` -- it evaluated the
# answer and threw it away, and the Encoding came back nil. Same shape as
# 96c32858's write payload.

a = ["abc", 1][0]
p a.encoding
p a.encoding.to_s
p a.encoding == Encoding::UTF_8

b = [[65].pack("C"), 1][0]
p b.encoding.to_s

h = { "k" => "abc" }
p h["k"].encoding.to_s

def enc_of(v) = v.encoding.to_s
puts enc_of("abc")
puts enc_of([65].pack("C"))

# the concrete arm still answers
p "abc".encoding
p "abc".encoding.to_s
