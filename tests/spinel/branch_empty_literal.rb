# An empty `[]` / `{}` caches TY_UNKNOWN because it has no ELEMENT type until
# something supplies one -- not because it has no value. Every branch emitter
# read that as "value-less tail", ran the literal for effect and left the slot
# at its default, and every inference site let ty_unify() drop it and answer
# whatever the OTHER arm said. So the branch answered nil, or "", or 0, or
# refused to compile when the sibling was an Integer.
#
# #4102 fixed the `case` arm merged with a sibling of the same container kind.
# This covers the rest: no sibling at all, a sibling of another kind, and the
# if / unless / ternary / case-in forms.
def s(l); v = yield; puts "#{l}: #{v.inspect}"; end

s("if both empty")   { if true then [] else [] end }
s("if no else")      { if true then [] end }
s("if vs string")    { if true then [] else "x" end }
s("if vs int")       { if true then [] else 1 end }
s("unless both")     { unless false then [] else [] end }
s("unless no else")  { unless false then [] end }
s("ternary both")    { true ? [] : [] }
s("ternary vs int")  { true ? [] : 1 }
s("case both empty") { case "s" when String then [] else [] end }
s("case no else")    { case "s" when String then [] end }
s("case vs string")  { case "s" when String then [] else "x" end }
s("case vs int")     { case "s" when String then [] else 1 end }
s("case hash both")  { case "s" when String then {} else {} end }
s("case hash vs s")  { case "s" when String then {} else "x" end }
s("casein both")     { case 1; in Integer; []; else; []; end }
s("casein vs int")   { case 1; in Integer; []; else; 1; end }
s("begin both")      { begin; []; rescue; []; end }

# what #4102 already covered stays covered
s("case vs concrete"){ case "s" when String then [] else ["x"] end }
s("case hash conc")  { case "s" when String then {} else { "a" => 1 } end }

# && / || answer the same question about the same literal
s("and")             { true && [] }
s("or")              { false || [] }

# the value is a real container, not one that only prints right
def use(l); v = yield; v << 9 if v.is_a?(Array); v["k"] = 9 if v.is_a?(Hash); puts "#{l}: #{v.inspect} size=#{v.size}"; end
use("if no else")    { if true then [] end }
use("case vs int")   { case "s" when String then [] else 1 end }
use("casein both")   { case 1; in Integer; []; else; []; end }
use("hash vs s")     { case "s" when String then {} else "x" end }
