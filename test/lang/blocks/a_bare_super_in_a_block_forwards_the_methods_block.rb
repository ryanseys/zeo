# A bare `super` written inside a BLOCK forwards the enclosing method's block
# to the parent, exactly as one written directly in the method body does.
# zeo forwards the method's PARAMETERS through this shape correctly -- see
# `test/lang/blocks/a_bare_super_inside_a_block_forwards_the_methods_params.rb` -- but
# not its BLOCK, so the parent raises `LocalJumpError: no block given`.
#
# The two halves are synthesized at different places. The forwarded argument
# list is built by `codegen::call::super_calls` at the emission site, which is
# inside the block and still knows the enclosing method's `Params`. The
# forwarded BLOCK is the enclosing method's `__blk` parameter, and whether the
# method has one at all is decided much earlier, by `analyze`'s
# `scan_bare_block_use` -- which does see the nested `super` and does set
# `uses_bare_block`. What is missing is the capture: `__blk` is an ordinary
# local of the enclosing method, so the block's `move` closure has to capture
# it the way it captures any other parameter the body reads, and nothing in
# the block's HIR mentions it (the reference is synthesized at emit time).
#
# That is the same shape as `Captures::zsuper_forwards`, which exists for
# precisely this reason on the parameter side: a bare `super` in a block makes
# the enclosing method's parameters live even though they appear nowhere in
# the block. The fix is to extend that flag to imply `__blk` as well, so the
# closure captures the block along with the parameters it already does.
#
# Written as one file so the divergence is the whole output: CRuby yields the
# block through both `super`s and prints the collected values.
class Base
  def each_thing
    yield 1
    yield 2
    :base_done
  end
end

class Nested < Base
  def each_thing
    [1].map { super }.first
  end
end

acc = []
p Nested.new.each_thing { |v| acc << v }
p acc

# The same through two closures, which is how it reaches a real gem: the
# parameters forward, so only the block is missing.
class Deeper < Base
  def each_thing
    [1].map { [2].map { super }.first }.first
  end
end

acc2 = []
p Deeper.new.each_thing { |v| acc2 << v * 10 }
p acc2
__END__
:base_done
[1, 2]
:base_done
[10, 20]
