# #552. Body-usage param inference widens int/nil-defaulted
# params to flat `poly` when the body calls a length-like
# method (length / size / empty?) on the param. These methods
# exist on String / Array / Hash but NOT on Integer, so a
# body proving it calls them on a param establishes the param
# is one of those container types. Codegen's poly-recv
# dispatch routes through tag/cls_id arms covering String +
# every Array variant + every Hash variant.
#
# Discriminator: the Hash case (last line, prints 2) failed
# pre-fix; the warning was silenced for some cases by earlier
# Option C work but the Hash-variant length dispatch wasn't
# wired in emit_poly_builtin_dispatch. String and Array
# variants happen to have worked already; this test still
# pins them so a future regression in the broader poly-recv
# length pathway is caught.
#
# nil semantics: spinel returns 0 for nil.length (the result
# temp's default) while CRuby raises NoMethodError. The
# silent emit-0 isn't fixed here; the test pins the
# spinel-specific behavior, not CRuby reference output.
#
# Conservative classifier: only length / size / empty? widen.
# These don't exist on Integer so widening to poly is sound.
# Methods that DO exist on Integer (<<, &, |, +, -, *, [])
# are deliberately excluded -- a body like
# `def poke(data); data << 8; end` (optcarrot's PPU) is
# legitimately Integer arithmetic and would break under a
# broader widening.

def consume_length(x)
  x.length
end

def consume_empty(x)
  x.empty?
end

class Box
  attr_accessor :contents
end

b = Box.new
puts consume_length(b.contents)
b.contents = "hello"
puts consume_length(b.contents)
b.contents = [1, 2, 3, 4]
puts consume_length(b.contents)
b.contents = { "a" => 1, "b" => 2 }
puts consume_length(b.contents)
puts consume_empty(b.contents)
b.contents = {}
puts consume_empty(b.contents)
