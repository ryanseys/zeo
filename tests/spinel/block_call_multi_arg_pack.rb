# A block invoked through `block.call(a, b)` with MULTIPLE arguments packs
# those arguments into an array for a collector that takes the block's value
# as one object (`to_a`), and binds them across a destructuring block's
# parameters -- exactly as `yield a, b` already does. Calling the block with
# several arguments dropped all but the first, so an Enumerable whose #each
# forwards through a &block parameter collected scalars where CRuby collects
# pairs.
#
# Found porting tobi/try: its fuzzy matcher yields (entry, positions, score)
# from an #each that forwards the caller's block, so `.to_a` silently lost the
# positions and the score.
class ViaCall
  include Enumerable
  def each(&block)
    block.call(1, 2)
    block.call(3, 4)
  end
end

# `yield` with the same arity, for contrast: this form already worked.
class ViaYield
  include Enumerable
  def each
    yield 1, 2
    yield 3, 4
  end
end

p ViaCall.new.to_a
p ViaYield.new.to_a

# A single-argument call still passes its value through unwrapped.
class Single
  include Enumerable
  def each(&block)
    block.call(1)
    block.call(2)
  end
end
p Single.new.to_a
