# `using Module.new { refine C do ... end }` is irb's shape, and the only way to
# activate a refinement over a class chosen where no constant names the module.
# The lowering already built the holder and the activation as a PAIR, and a
# statement is one node -- so at the top level the pair was dropped on the floor
# and `using` ran as an ordinary call, which raises. It rides in a `Seq` now.
#
# `test/lang/refinements/refinement_holder_names_reach_codegen.rb` covers the holder's NAME
# reaching a Rust ident; this covers the holder being activated at all.
Named = 1

using Module.new {
  refine Array do
    def second = self[1]
    def penultimate = self[-2]
  end

  refine Symbol do
    def shout = to_s.upcase + "!"
  end
}

p [10, 20, 30].second
p [10, 20, 30].penultimate
p :hi.shout

# A second anonymous holder: the two are named for their byte offsets, so they
# must stay distinct and both stay active.
using Module.new { refine Integer do def kb = self * 1024 end }
p 2.kb
p [10, 20, 30].second

# The holder claims no constant, so `Named` still means what it did.
p Named
__END__
20
20
"HI!"
2048
20
1
