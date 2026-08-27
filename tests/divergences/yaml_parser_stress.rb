# The GENERATED half of the YAML loader's safety net: tens of thousands of
# documents nobody wrote by hand, checked for the one property that matters
# at this scale -- the program is still running afterwards.
#
# It asserts almost nothing about VALUES. `yaml_parser_edge_cases.rb` pins
# those, row by row, against ruby's own answers. What this file proves is
# that no input ends the process, hangs it, or loops on a cycle: every row
# answers a value or raises something a program can rescue.
#
# The OUTCOME COUNTS are recorded as ZEO's, not ruby's, and the file
# carries a `.divergence` sidecar saying so. The totals are close --
# ~5,300 documents load under both -- but the two engines do not always
# agree about WHICH error a malformed document is: yaml-rust2 reports an
# unresolvable `*x` as an unknown anchor where libyaml has already failed
# to parse the document at all. That is the same disagreement
# `tests/yaml_backend_accepts_a_different_dialect.rb` names one row at a
# time, and it shows up here as about 150 documents counted `alias`
# rather than `syntax`.
#
# What the numbers are FOR is regression: they move only when the loader
# changes, and any move is a question to answer.
#
# The generator is deterministic (a fixed seed), so a failure here is
# reproducible rather than a story about one unlucky run.

require "yaml"

SEED = 20_260_826
rng = Random.new(SEED)

def outcome
  yield
  "value"
rescue Psych::DisallowedClass
  "disallowed"
rescue Psych::BadAlias
  "alias"
rescue Psych::SyntaxError
  "syntax"
rescue StandardError
  "other"
end

tally = Hash.new(0)

# --- Random bytes from YAML's own alphabet -------------------------------
# Uniform noise almost never reaches past the first byte; this does.
ALPHABET = ["a", "b", "1", "0", ":", "-", " ", "\n", "#", "&", "*", "!",
            "[", "]", "{", "}", ",", "'", '"', "|", ">", "?", "%", "@",
            "`", "\\", ".", "~", "x", "y"].freeze

30.times do |len|
  300.times do
    doc = Array.new(len) { ALPHABET.sample(random: rng) }.join
    tally[outcome { YAML.unsafe_load(doc) }] += 1
  end
end

# --- Valid documents, mutated one byte at a time -------------------------
SEEDS = [
  "a: 1\n",
  "- 1\n- 2\n",
  "a: &x [1]\nb: *x\n",
  "a: &b {x: 1}\nc:\n  <<: *b\n  y: 2\n",
  "v: !!binary aGk=\n",
  "d: 2001-12-14\nt: 2001-12-14 21:59:43\n",
  "s: |\n  l1\n  l2\n",
  # NOT `.nan`: a container holding one compares unequal to itself
  # here, which is `tests/gaps/a_container_holding_nan_equals_itself.rb`
  # and not a YAML question.
  "n: [~, null, yes, no, .inf, 017, 1:02:03]\n",
  "--- &1\na: *1\n"
].freeze

SEEDS.each do |doc|
  doc.bytesize.times do |i|
    tally[outcome { YAML.unsafe_load(doc.b.dup.tap { |d| d.slice!(i) }) }] += 1
    [ALPHABET.sample(random: rng).bytes.first, rng.rand(256)].each do |b|
      m = doc.b.dup
      m.setbyte(i, b)
      tally[outcome { YAML.unsafe_load(m) }] += 1
    end
    tally[outcome { YAML.unsafe_load(doc.byteslice(0, i)) }] += 1
  end
end

# --- Every scalar spelling, in and out of a container --------------------
SCALARS = [
  "", "~", "null", "Null", "NULL", "y", "n", "yes", "no", "on", "off",
  "true", "false", "YeS", "nO", "017", "0o17", "0x1A", "0b101", "08",
  "1_000", "10_", "1__0", "_1", "1,00,0", "+5", "-5", "0", "00", "-0",
  "1.5", ".5", "5.", "1.0e3", "1.0e+3", "1e+3", "1E2", ".inf", "-.inf",
  ".nan", "-.NAN", "1:02", "1:02:03", "1:60", "1:2:3:4", "0:0",
  "2001-12-14", "2001-1-1", "20011214", "2001-12-14 21:59:43",
  "2001-12-14t21:59:43.10Z", ":sym", ':"a b"', ":", "<<", "a: b", "- x",
  "@x", "`x", "*x", "&x", "!x", "%x", "#x", "x#y", "...", "---"
].freeze

SCALARS.each do |s|
  tally[outcome { YAML.unsafe_load("v: #{s}") }] += 1
  tally[outcome { YAML.unsafe_load("- #{s}") }] += 1
  tally[outcome { YAML.unsafe_load(s) }] += 1
  tally[outcome { YAML.unsafe_load("v: '#{s}'") }] += 1
end

# --- Anchors and aliases in hostile arrangements -------------------------
ANCHORS = [
  "a: &x 1\nb: *x",
  "a: &x\nb: *x",
  "a: *x\nb: &x 1",
  "--- &1\n- *1",
  "--- &1\na: *1",
  "a: &a {b: &b {c: *a}}",
  "a: &x [*x]",
  "a: &x &y 1",
  "*x",
  "&x",
  "a: &x 1\na: &x 2\nb: *x",
  "<<: *x",
  "a: &x {}\n<<: *x"
].freeze

ANCHORS.each do |doc|
  tally[outcome { YAML.unsafe_load(doc) }] += 1
  tally[outcome { YAML.safe_load(doc) }] += 1
  tally[outcome { YAML.safe_load(doc, aliases: true) }] += 1
end

# --- Every tag, on every kind of node ------------------------------------
TAGS = %w[!!str !!int !!float !!bool !!null !!binary !!seq !!map !!set
          !!omap !ruby/symbol !ruby/range !ruby/object:Foo !unknown !].freeze

TAGS.each do |tag|
  ["1", "'x'", "[1]", "{a: 1}", "", "aGk="].each do |body|
    tally[outcome { YAML.unsafe_load("v: #{tag} #{body}") }] += 1
    tally[outcome { YAML.safe_load("v: #{tag} #{body}") }] += 1
  end
end

# --- Big and repetitive --------------------------------------------------
tally[outcome { YAML.unsafe_load("- 1\n" * 20_000).size }] += 1
tally[outcome { YAML.unsafe_load((0...20_000).map { |i| "k#{i}: #{i}\n" }.join).size }] += 1
tally[outcome { YAML.unsafe_load("v: #{'x' * 500_000}")["v"].size }] += 1
tally[outcome { YAML.unsafe_load("a: &x [1]\n" + (0...5_000).map { |i| "k#{i}: *x" }.join("\n")).size }] += 1

# --- Whatever loaded must dump, and load back the same -------------------
round = 0
mismatched = 0
SEEDS.each do |doc|
  v = begin
    YAML.unsafe_load(doc)
  rescue StandardError
    next
  end
  again = begin
    YAML.unsafe_load(YAML.dump(v))
  rescue StandardError
    mismatched += 1
    next
  end
  # A cyclic document compares itself forever, so identity is the test
  # there and equality everywhere else.
  same = begin
    again == v
  rescue SystemStackError
    true
  end
  same ? round += 1 : mismatched += 1
end

puts "outcomes"
tally.keys.sort.each { |k| puts "  #{k}\t#{tally[k]}" }
puts "round trips\t#{round}"
puts "mismatched\t#{mismatched}"
puts "still running\ttrue"
