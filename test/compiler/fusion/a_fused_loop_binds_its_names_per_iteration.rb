# `n.times { |i| }` and `(a..b).each { |i| }` on a LITERAL receiver splice
# into a counted loop with no frame of their own. Ruby still binds the block's
# names PER INVOCATION, so a closure built in one iteration must keep what it
# captured when the next iteration rebinds -- the splice cannot hand every
# closure one shared slot.
#
# CLIF gave the spliced param a plain slot and REFUSED any escaping block that
# captured it ("the shadow dies with the loop"), which took out
# `3.times { |i| Process.fork { exit i } }` among others. A name a closure
# escapes with now gets a fresh cell each iteration; a name nothing escapes
# with keeps the slot, and the uncaptured loop is untouched.

# --- the parameter ----------------------------------------------------------
procs = []
3.times { |i| procs << -> { i } }
p procs.map(&:call)

ranged = []
(1..3).each { |i| ranged << -> { i * 10 } }
p ranged.map(&:call)

# --- a name first assigned inside the body ---------------------------------
doubled = []
3.times do |i|
  n = i * 2
  doubled << -> { n }
end
p doubled.map(&:call)

# --- an explicit block-local (`|i; n|`) ------------------------------------
n = "outer"
labels = []
3.times do |i; n|
  n = "in#{i}"
  labels << -> { n }
end
p labels.map(&:call)
p n

# --- a block-local that is never assigned is nil each time ------------------
seen = []
2.times { |i; fresh| seen << fresh; fresh = i }
p seen

# --- a conditional first assignment does not carry over --------------------
carried = []
3.times do |i|
  once = i if i == 1
  carried << once
end
p carried

# --- the param shadows an enclosing local of the same name -----------------
i = :outer
2.times { |i| }
p i

# --- nested splices: each level binds its own ------------------------------
pairs = []
2.times do |a|
  2.times do |b|
    pairs << -> { [a, b] }
  end
end
p pairs.map(&:call)

# --- a splice inside a real block ------------------------------------------
mixed = []
[10, 20].each do |base|
  2.times { |k| mixed << -> { base + k } }
end
p mixed.map(&:call)

# --- the uncaptured fast path is untouched ---------------------------------
total = 0
5.times { |x| total += x }
p total

acc = []
(0..4).each do |x|
  next if x == 2
  break if x == 4
  acc << x
end
p acc

# --- a closure captured mid-loop sees the value at capture time ------------
snapshots = []
3.times do |i|
  snapshots << -> { i }
  i = i + 100
end
p snapshots.map(&:call)
__END__
[0, 1, 2]
[10, 20, 30]
[0, 2, 4]
["in0", "in1", "in2"]
"outer"
[nil, nil]
[nil, 1, nil]
:outer
[[0, 0], [0, 1], [1, 0], [1, 1]]
[10, 11, 20, 21]
10
[0, 1, 3]
[100, 101, 102]
