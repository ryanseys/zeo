# A Regexp is not a GC object: it is a compiled program the regexp engine
# mallocs, which sp_mark_rbval already excludes from sp_gc_mark. A closure cell
# holding one missed that exclusion, so marking a captured Regexp read a GC
# header one byte in front of the engine's allocation and the collector faulted
# on whatever it found -- SIGBUS from sp_gc_mark_all, or a heap-buffer-overflow
# under -fsanitize=address (#4063). The cell itself is GC-allocated and keeps
# itself alive; the pattern it points at is never collected.
WORDS = ["alpha", "gamma"]

def lines_of(i)
  Array.new(20) { |j| "file #{i} line #{j} mentions alpha and beta" }
end

total = 0
60.times do |i|
  matchers = WORDS.map do |text|
    re = Regexp.new("\\b#{text}\\b")
    ->(line) { !re.match(line).nil? }
  end
  matchers.push(->(line) { line.include?("beta") })
  lines_of(i).each { |line| matchers.each { |m| total += 1 if m.call(line) } }
end
p total

# the captured pattern still matches after many collections, and reassigning
# the captured cell is still visible to the closure
pat = Regexp.new("z+")
probe = ->(s) { pat.match(s).nil? ? "no" : "yes" }
100.times { Array.new(20) { |k| "junk #{k}" } }
p probe.call("azzb")
pat = Regexp.new("q+")
p probe.call("azzb")
p probe.call("aqqb")
