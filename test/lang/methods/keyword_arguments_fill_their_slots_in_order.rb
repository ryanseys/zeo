# A keyword call whose keys are literal and cover the callee's required
# keywords EXACTLY can fill the parameter slots at the call site. The two
# orders it has to keep apart:
#
#   - keyword VALUES evaluate in WRITTEN order (they are ordinary argument
#     expressions, and one of them can have a side effect)
#   - they land in the callee's DECLARED order (which is the slot order)
#
# Every other pairing -- a missing key, a spare one, a `**` splat, a
# non-literal key -- is the binder's question, and stays its question so
# its error text cannot drift.
def add(left:, right:) = left + right
def mix(a, b, k:, j:) = [a, b, k, j]

p add(left: 1, right: 2)
p add(right: 2, left: 1)
p mix(1, 2, j: 4, k: 3)

$log = []
def note(x) = ($log << x; x)
p mix(note(:a), note(:b), j: note(:j), k: note(:k))
p $log

begin
  add(left: 1)
rescue ArgumentError => e
  puts "missing: #{e.message}"
end
begin
  add(left: 1, right: 2, extra: 3)
rescue ArgumentError => e
  puts "extra: #{e.message}"
end

h = { left: 1, right: 2 }
p add(**h)
p add(**{ left: 5, right: 6 })
key = :left
p add(key => 1, right: 9)

# An OPTIONAL keyword keeps the binder -- deciding what is absent is its
# whole job -- and so does a `**rest`.
def opt(a:, b: a * 10) = [a, b]
def rest(a:, **others) = [a, others]
p opt(a: 1)
p opt(a: 1, b: 2)
p rest(a: 1, z: 26)
__END__
3
3
[1, 2, 3, 4]
[:a, :b, :k, :j]
[:a, :b, :j, :k]
missing: missing keyword: :right
extra: unknown keyword: :extra
3
11
10
[1, 10]
[1, 2]
[1, {z: 26}]
