# A block handed to a method that FORWARDS it to a self-recursive yielder.
#
# The yielder recurses, so its block cannot be spliced into it -- it is lowered
# to take a real &block proc. The forwarder in front of it is marked `yields`
# all the same (forwarding is what earns that mark), and the capture analysis
# read that mark as "the block is spliced, so nothing escapes". The block was
# then built as a proc with no capture struct, and its writes to enclosing
# locals went to a copy of them: the block ran, and what it assigned never
# arrived (#4145).
$ran = 0

def walk(n)
  yield
  walk(n - 1) { nil } if n > 0
  nil
end

def hop(n, &block)
  walk(n, &block)
end

seen = nil
hop(1) { $ran += 1; seen = 1 }
p [seen, $ran]

# Deeper: two forwarders in front of the yielder, and a capture written on
# every recursion level rather than once.
$log = []

def down(n)
  yield n
  down(n - 1) { |k| $log << -k } if n > 0
  nil
end

def mid(n, &b) = down(n, &b)
def top(n, &b) = mid(n, &b)

acc = []
top(2) { |k| acc << k }
p [acc, $log]

# The block written at the yielder's own call site was always right; pinned so
# the two paths cannot drift apart.
direct = []
walk(1) { direct << :d }
p direct

# The return type of a lowered recursive yielder is the one its body derives.
# It used to be forced to Integer, which was right only by coincidence: a body
# ending in a String or an Array emitted `return char *` from a function typed
# sp_int and produced no binary at all.
def ends_nil(n)
  yield
  ends_nil(n - 1) { nil } if n > 0
  nil
end

def ends_str(n)
  yield
  ends_str(n - 1) { nil } if n > 0
  "x"
end

def ends_ary(n)
  yield
  ends_ary(n - 1) { nil } if n > 0
  [1]
end

def ends_sym(n)
  yield
  ends_sym(n - 1) { nil } if n > 0
  :a
end

p [ends_nil(1) { }, ends_str(1) { }, ends_ary(1) { }, ends_sym(1) { }]

# A return that DOES come out of a yield still reads the block's answer through
# the proc side-channel, which is what the forced Integer was there for.
def carry(n)
  return yield if n <= 0
  carry(n - 1) { yield }
end
p carry(2) { 42 }
