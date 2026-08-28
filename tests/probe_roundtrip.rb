# Differential probe: ROUND-TRIP INTEGRITY.
#
# For every serializer and every container, asserts that a value survives
# its own encoding -- `load(dump(x)) == x`, sizes preserved, key identity
# preserved. A GOLDEN: the `.expected` beside it is ruby 4.0.6's own
# answers.
#
# This is the highest-yield way to catch SILENT corruption, because it needs
# no reference output to compare against: a value that does not survive its
# own serializer is wrong on its own terms, and the row says so on both
# engines. The differential half then catches the cases where zeo and ruby
# BOTH answer, differently.
#
# The Hash key-identity rows exist because a table keyed on a user `hash`
# alone silently held one entry where two were stored, and read back
# another key's value.

require "json"
require "yaml"
require "set"
require "stringio"

ROWS = {}

def probe(name, &blk) = ROWS[name] = blk

# Rows zeo does not answer yet, each naming the gap that tracks it. A carved
# row is left OUT of the output rather than recorded wrong. When a gap is
# promoted, delete its entry here and re-bless; the row comes back.
SKIP = {
  "yaml:string_binary" => "tests/gaps/a_binary_string_survives_a_yaml_round_trip.rb",
  "marshal array ivar" => "tests/gaps/marshal_carries_an_array_s_ivars.rb",
}.freeze

VALUES = {
  "nil" => nil,
  "true" => true,
  "int" => 42,
  "negzero" => -0,
  "bignum" => 2**70,
  "float" => 1.5,
  "float_e" => 1e100,
  "string" => "hi",
  "string_utf8" => "héllo",
  "string_binary" => "\xff\xfe".b,
  "symbol" => :sym,
  "array" => [1, [2, [3]]],
  "hash" => { "a" => 1, "b" => { "c" => 2 } },
  "empty" => [],
  "nested_empty" => { "a" => [], "b" => {} },
}.freeze

VALUES.each do |name, v|
  probe("marshal:#{name}") { Marshal.load(Marshal.dump(v)) == v }
end

VALUES.each do |name, v|
  # JSON has no Symbol or binary-string tier, so those rows read as their
  # JSON projection rather than as equality -- still a round trip, of the
  # thing JSON actually carries.
  probe("json:#{name}") { JSON.parse(JSON.generate([v])).first.inspect }
end

VALUES.each do |name, v|
  probe("yaml:#{name}") { YAML.unsafe_load(YAML.dump(v)) == v }
end

# --- Structural sharing ---------------------------------------------------
# The CYCLE rows live at the very bottom: they are the ones that can take the
# process down, and every row after an abort reads as `<no row>` rather than
# as a finding.
probe("marshal shared") do
  inner = [1]
  outer = [inner, inner]
  r = Marshal.load(Marshal.dump(outer))
  r[0].equal?(r[1])
end
probe("yaml shared") do
  inner = [1]
  r = YAML.unsafe_load(YAML.dump([inner, inner]))
  r[0].equal?(r[1])
end

# --- Ivars and extensions ride along --------------------------------------
probe("marshal string ivar") do
  s = +"hi"
  s.instance_variable_set(:@meta, 7)
  Marshal.load(Marshal.dump(s)).instance_variable_get(:@meta)
end
probe("marshal array ivar") do
  a = [1]
  a.instance_variable_set(:@meta, 7)
  Marshal.load(Marshal.dump(a)).instance_variable_get(:@meta)
end
probe("marshal object ivars") do
  c = Struct.new(:a)
  Marshal.load(Marshal.dump(c.new(1))).a
end
probe("marshal encoding") do
  Marshal.load(Marshal.dump("héllo")).encoding.to_s
end
probe("marshal binary encoding") do
  Marshal.load(Marshal.dump("\xff".b)).encoding.to_s
end

# --- Hash key identity ----------------------------------------------------
class Colliding
  attr_reader :n

  def initialize(n) = @n = n
  def hash = 7
  def eql?(o) = o.is_a?(Colliding) && o.n == @n
  def ==(o) = eql?(o)
end

class OnlyHash
  def hash = 7
end

class OnlyEql
  def eql?(_o) = true
  def ==(_o) = true
end

probe("hash colliding size") do
  h = {}
  h[Colliding.new(1)] = "a"
  h[Colliding.new(2)] = "b"
  h.size
end
probe("hash colliding read") do
  h = {}
  h[Colliding.new(1)] = "a"
  h[Colliding.new(2)] = "b"
  h[Colliding.new(1)]
end
probe("hash colliding key?") do
  h = { Colliding.new(1) => "a" }
  [h.key?(Colliding.new(1)), h.key?(Colliding.new(2))]
end
probe("hash colliding delete") do
  h = {}
  h[Colliding.new(1)] = "a"
  h[Colliding.new(2)] = "b"
  h.delete(Colliding.new(1))
  [h.size, h.values]
end
probe("uniq colliding") { [Colliding.new(1), Colliding.new(2), Colliding.new(1)].uniq.size }
probe("set colliding") { Set.new([Colliding.new(1), Colliding.new(2), Colliding.new(1)]).size }
probe("group_by colliding") do
  [1, 2, 3].group_by { Colliding.new(1) }.size
end
probe("only hash") do
  h = {}
  h[OnlyHash.new] = 1
  h[OnlyHash.new] = 2
  h.size
end
probe("only eql") do
  h = {}
  h[OnlyEql.new] = 1
  h[OnlyEql.new] = 2
  h.size
end
probe("compare_by_identity") do
  k = Colliding.new(1)
  h = {}.compare_by_identity
  h[k] = 1
  h[Colliding.new(1)] = 2
  [h.size, h[k]]
end
probe("string key dup") do
  h = {}
  h[+"k"] = 1
  h[+"k"] = 2
  [h.size, h["k"]]
end
probe("frozen string key") do
  h = { "k" => 1 }
  h.keys.first.frozen?
end
probe("float int keys") do
  h = { 1 => "int", 1.0 => "float" }
  [h.size, h[1], h[1.0]]
end
probe("array key") do
  h = { [1, [2]] => "v" }
  h[[1, [2]]]
end
probe("hash key") do
  h = { { "a" => 1 } => "v" }
  [h[{ "a" => 1 }], h[{ "b" => 1 }].inspect]
end
probe("range key") { { (1..2) => "v" }[(1..2)] }
probe("regexp key") { { /a/i => "v" }[/a/i] }
probe("mutated key") do
  k = +"k"
  h = { k => 1 }
  k << "!"
  [h.size, h["k"].inspect, h["k!"].inspect, h.rehash && h["k!"]]
end

# --- StringIO round trips -------------------------------------------------
probe("stringio append") do
  s = StringIO.new(+"abc", "a")
  s.write("d")
  s.string
end
probe("stringio truncate mode") do
  s = StringIO.new(+"abc", "w")
  s.string
end
probe("stringio read only write") do
  StringIO.new(+"abc", "r").write("d")
end
probe("stringio append read") do
  StringIO.new(+"abc", "a").read
end

# --- Cycles: LAST, because these are the rows that can abort the process ---
probe("marshal cycle") do
  a = []
  a << a
  r = Marshal.load(Marshal.dump(a))
  r[0].equal?(r)
end
probe("json cycle") do
  a = []
  a << a
  JSON.generate(a)
end
probe("yaml cycle") do
  a = []
  a << a
  r = YAML.unsafe_load(YAML.dump(a))
  r[0].equal?(r)
end

ROWS.each do |name, fn|
  next if SKIP.key?(name)

  r = begin
    v = fn.call
    "ok #{v.inspect}"
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts "#{name}\t#{r}"
  $stdout.flush
end
