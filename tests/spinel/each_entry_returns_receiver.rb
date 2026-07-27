# Enumerable#each_entry answers the receiver, like #each.
#
# It had no emitter at all for a Hash or a Range, and answered nil for an Array
# and a Dir, so chaining off the result raised NoMethodError on a value that in
# Ruby is the enumerable itself.
#
# On any receiver whose #each yields ONE value per element -- every builtin
# enumerable -- each_entry IS each, so it is renamed to one. A user Enumerable
# is left alone: its #each may `yield a, b`, and there the two differ (see
# docs/limitations.md).

# our own directory, not a platform-specific one; the name never reaches the
# output, only the entries do
dir = "/tmp/sp_each_entry_#{Process.pid}"
Dir.mkdir(dir) unless Dir.exist?(dir)
File.write(File.join(dir, "a.txt"), "x")

d = Dir.new(dir)
p d.each_entry { |e| }.class
d.close

d = Dir.new(dir)
seen = []
d.each_entry { |e| seen.push(e) }
p seen.sort
d.close

File.delete(File.join(dir, "a.txt"))
Dir.rmdir(dir)

# the builtin enumerables, each answering itself
arr = [1, 2, 3]
p arr.each_entry { |x| }.class
p(arr.each_entry { |x| }.equal?(arr))

h = { "a" => 1, "b" => 2 }
p h.each_entry { |pair| }.class

r = (1..3)
p r.each_entry { |x| }.class

# the elements are what #each yields, in order
acc = []
arr.each_entry { |x| acc.push(x * 2) }
p acc

pairs = []
h.each_entry { |k, v| pairs.push([k, v]) }
p pairs

# a user Enumerable keeps its own path and still answers itself
class Trio
  include Enumerable

  def each
    yield 1
    yield 2
    yield 3
  end
end

t = Trio.new
p t.each_entry { |x| }.class
got = []
t.each_entry { |x| got.push(x) }
p got
