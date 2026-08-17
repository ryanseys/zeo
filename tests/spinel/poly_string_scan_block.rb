# `str.scan(pat) { }` where the receiver is only known to be a String at run
# time, and the pattern is a Regexp read out of a table (so it arrives boxed).
# The poly path had no scan arm at all and raised NoMethodError.

PATTERNS = { "num" => /\d+/, "word" => /[a-z]+/ }

def pick(f, s)
  f ? s : 7
end

log = pick(true, "abc 123 def 45")
counts = Hash.new(0)
PATTERNS.each do |name, regex|
  log.scan(regex) { counts[name] += 1 }
end
p counts["num"]
p counts["word"]

seen = []
log.scan(/\d+/) { |m| seen << m }
p seen

# the same, with the receiver statically a String and only the pattern boxed
class L
  def initialize
    @log = "abc 123 def 45"
  end

  def run
    counts = Hash.new(0)
    PATTERNS.each do |name, regex|
      @log.scan(regex) { counts[name] += 1 }
    end
    counts.values.sum
  end
end

p L.new.run

# the blockless form with the same boxed pattern
PATTERNS.each do |name, regex|
  p [name, log.scan(regex).size]
end
