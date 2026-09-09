# The GENERATED half of the parser's safety net: tens of thousands of
# inputs nobody wrote by hand, checked for the one property that matters at
# this scale -- the program is still running afterwards.
#
# It asserts almost nothing about VALUES. `json_parser_edge_cases.rb` pins
# those, row by row, against ruby's own answers. What this file proves is
# that no input ends the process, hangs it, or reads past the end of a
# buffer: every row answers a value or raises something a program can
# rescue, and the counts at the end are the same under both engines.
#
# The generator is deterministic (a fixed seed), so a failure here is
# reproducible rather than a story about one unlucky run.
#
# Two shapes found real defects when this was written: a `\u` escape
# truncated by the end of input reported the wrong error, and a stray NUL
# after a document quoted a byte ruby does not.

require "json"

SEED = 20_260_826
rng = Random.new(SEED)

# `outcome` collapses a run to one of four words, so the counts can be
# compared without pinning a message that is allowed to differ.
def outcome
  yield
  "value"
rescue JSON::NestingError
  "nesting"
rescue JSON::ParserError
  "parser"
rescue StandardError => e
  "other:#{e.class}"
end

tally = Hash.new(0)

# --- Random bytes, every length from empty to sixty-four -----------------
64.times do |len|
  200.times do
    bytes = Array.new(len) { rng.rand(256) }.pack("C*")
    tally[outcome { JSON.parse(bytes) }] += 1
  end
end

# --- Random bytes drawn only from JSON's own alphabet --------------------
# Far more likely to reach deep into the parser than uniform noise.
ALPHABET = %w<{ } [ ] " : , 0 1 9 - + . e E t f n u l r s a i \\ / * space
              tab newline>.map do |w|
  case w
  when "space" then " "
  when "tab" then "\t"
  when "newline" then "\n"
  else w
  end
end

40.times do |len|
  300.times do
    s = Array.new(len) { ALPHABET.sample(random: rng) }.join
    tally[outcome { JSON.parse(s) }] += 1
  end
end

# --- Valid documents, mutated one byte at a time -------------------------
SEEDS = [
  '{"a":1}',
  '[1,2,3]',
  '{"a":[1,{"b":null}],"c":"é"}',
  '[-1.5e-3,true,false,null,""]',
  '{"k":"😀","n":123456789012345678901234567890}',
  '[[[[[1]]]]]',
  '{"a":{"b":{"c":{"d":[]}}}}'
].freeze

SEEDS.each do |doc|
  doc.bytesize.times do |i|
    # Deleted.
    tally[outcome { JSON.parse(doc.b.dup.tap { |d| d.slice!(i) }) }] += 1
    # Replaced by a byte from the alphabet, and by a random one.
    [ALPHABET.sample(random: rng).bytes.first, rng.rand(256)].each do |b|
      m = doc.b.dup
      m.setbyte(i, b)
      tally[outcome { JSON.parse(m) }] += 1
    end
    # Truncated here.
    tally[outcome { JSON.parse(doc.byteslice(0, i)) }] += 1
  end
end

# --- Numbers at every boundary a parser has ------------------------------
NUMBERS = [
  "0", "-0", "0.0", "-0.0", "1e0", "1E0", "1e+0", "1e-0",
  "9223372036854775807", "9223372036854775808", "-9223372036854775808",
  "-9223372036854775809", "1" + "0" * 400, "0." + "0" * 400 + "1",
  "1e308", "1e309", "1e-323", "1e-324", "1e999999999", "-1e999999999",
  "1e" + "9" * 40, "0e0", "-0e-0", "00", "01", "1.", ".1", "+1", "1e",
  "1e+", "--1", "1-1", "0x1", "1_000", "Infinity", "-Infinity", "NaN"
].freeze

NUMBERS.each do |n|
  tally[outcome { JSON.parse("[#{n}]") }] += 1
  tally[outcome { JSON.parse(n) }] += 1
  tally[outcome { JSON.parse("[#{n}]", allow_nan: true) }] += 1
end

# --- Escapes at every boundary -------------------------------------------
%w[a b f n r t u v 0 / \\ " ' x z].each do |e|
  tally[outcome { JSON.parse(%Q{"\\#{e}"}) }] += 1
end
(0..0xFFFF).step(97) do |cp|
  tally[outcome { JSON.parse(format('"\\u%04x"', cp)) }] += 1
end

# --- Nesting, either side of every limit ---------------------------------
# 2,001 is NOT here: past zeo's stack ceiling the two engines deliberately
# disagree, and `test/divergences/json_nesting_is_bounded_by_the_stack.rb` is where
# that row lives. Everything below the ceiling must agree exactly.
[0, 1, 2, 99, 100, 101, 1_999, 2_000].each do |n|
  doc = "[" * n + "]" * n
  tally[outcome { JSON.parse(doc) }] += 1
  tally[outcome { JSON.parse(doc, max_nesting: false) }] += 1
  tally[outcome { JSON.parse("[" * n + "1" + "]" * n, max_nesting: false) }] += 1
end

# --- Big and repetitive --------------------------------------------------
tally[outcome { JSON.parse("[#{Array.new(50_000, 1).join(',')}]").size } ] += 1
tally[outcome { JSON.parse(%Q{"#{'x' * 500_000}"}).size }] += 1
tally[outcome { JSON.parse("{#{Array.new(20_000) { |i| %Q{"k#{i}":#{i}} }.join(',')}}").size }] += 1
tally[outcome { JSON.parse("[" + '"\\n"' * 50_000 + "]").size }] += 1

# --- Round trip: whatever parses must generate and parse back the same ---
round = 0
SEEDS.each do |doc|
  v = JSON.parse(doc)
  round += 1 if JSON.parse(JSON.generate(v)) == v
  round += 1 if JSON.parse(JSON.pretty_generate(v)) == v
  round += 1 if JSON.parse(JSON.generate(v, ascii_only: true)) == v
end

puts "outcomes"
tally.keys.sort.each { |k| puts "  #{k}\t#{tally[k]}" }
puts "round trips\t#{round}"
puts "still running\ttrue"
__END__
outcomes
  nesting	2
  parser	25328
  value	920
round trips	21
still running	true
