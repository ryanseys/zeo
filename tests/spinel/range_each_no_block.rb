# Range#each without a block returns an Enumerator; spinel materializes
# the element array so chaining works.
p((1..3).each.to_a)
p((1...4).each.to_a)
p((5..8).each.to_a.length)
