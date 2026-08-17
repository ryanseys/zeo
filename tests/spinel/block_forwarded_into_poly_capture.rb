# A block whose target hands it on to a poly receiver.
#
# `TreeNode#each` is yield-inlined -- its `&blk` calls are spliced at the call
# site -- so nothing marked the block as escaping. But the spliced body ends in
# `k.each(&blk)` on a poly receiver, and THAT dispatch materializes the block
# as a real proc. Without cells for its captures the emitter had to refuse the
# program; and the dispatch itself tried to build a fresh proc literal out of
# the `&blk` forwarding rather than passing the proc it already names.
#
# The same shape through a constant receiver: a constant that names no class is
# an ordinary value, so it is typed like any other receiver.

class TreeNode
  def initialize(v)
    @v = v
    @kids = []
  end

  def add(n)
    @kids << n
    self
  end

  def each(&blk)
    blk.call(@v)
    @kids.each { |k| k.each(&blk) }
  end
end

def pick(flag)
  flag ? TreeNode.new(1).add(TreeNode.new(2)) : [3, 4]
end

src = pick(false)
total = 0
src.each do |x|
  total += x
end
p total

src2 = pick(true)
seen = []
src2.each do |x|
  seen << x
end
p seen

SRC = pick(true)
CONFIG = begin
  hash = {}
  SRC.each do |x|
    hash[x] = x * 2
  end
  hash
end
p CONFIG.length
p CONFIG[2]
