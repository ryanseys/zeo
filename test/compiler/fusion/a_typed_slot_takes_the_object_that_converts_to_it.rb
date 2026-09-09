require "tmpdir"
ZTMP = Dir.mktmpdir

class Sep
  def to_str
    "-"
  end
end

class Idx
  def to_int
    1
  end
end

def err
  yield
rescue StandardError => e
  [e.class, e.message]
end

p "a-b-".chomp(Sep.new)
s = String.new("a-b")
p [s.chomp!(Sep.new), s]
p "a".ljust(4, Sep.new)
p "a".rjust(4, Sep.new)
p "a".center(5, Sep.new)
p "a-b".index(Sep.new, 0)
p "a-b-".rindex(Sep.new)
p "a-b-".rindex(Sep.new, 2)
p "\xff".scrub(Sep.new) == "-"
p String.new("\xff").scrub!(Sep.new) == "-"
p "a-b-c".split(Sep.new, 2)
p "a-b-c".split(Sep.new)
p "a".upto(Sep.new).to_a.size
class Fmt
  def to_str
    "a"
  end
end
p "ab".unpack1(Fmt.new)
p String.new("abc").bytesplice(0, 1, Sep.new)
p Time.utc(2000).strftime(Sep.new)
p String.new(Sep.new)
p Regexp.escape(Sep.new)
p format(Sep.new)
p sprintf(Sep.new)
ENV["SPINEL_SLOT"] = "v"
class Key
  def to_str
    "SPINEL_SLOT"
  end
end
p ENV[Key.new]
p ENV.key?(Key.new)
p ENV.store(Key.new, "w")
p ENV.delete(Key.new)
class Cmd
  def to_str
    "true"
  end
end
p system(Cmd.new)
p err { "abc".rindex(1) }
p err { "abc".ljust(4, 1) }
p err { "a-b".chomp(1) }
p err { "abc".split(1) }
p err { format(1) }
p err { Time.utc(2000).strftime(1) }
p err { Regexp.escape(1) }
p err { String.new(1) }

a = [1, 2, 3]
p [a.slice!(Idx.new, Idx.new), a]
p 8[Idx.new]
p 1 << Idx.new
p 8 >> Idx.new
p Time.at(Idx.new).to_i
p Time.utc(2000, Idx.new, Idx.new).month
p Time.local(2000, Idx.new, Idx.new).day
p Random.new(1).rand(Idx.new).class
p Random.rand(Idx.new).class
p [1, 2].each_slice(Idx.new).to_a
p [[1, 2], 3].dig(Idx.new)
m = /(a)(b)/.match("ab")
p [m.begin(Idx.new), m.end(Idx.new), m.offset(Idx.new), m.byteoffset(Idx.new), m.values_at(Idx.new)]
p /a/.match?("ba", Idx.new)
p err { [1, 2].cycle("x") { } }
p err { Random.rand("x") }
p err { a.slice!("x", 1) }
p err { 8["x"] }
p err { [1, 2].each_slice("x") { }; :no }
p err { Time.at([1]) }
p err { File.utime("x", "x", "nope") }
p err { 1.0.quo("x") }
p err { Math.sqrt("x") }
p err { Math.sqrt(nil) }
p err { Math.sqrt(Sep.new) }

h = { 1 => 2 }
p h.dig("a")
p h.dig(1.0)
p h.fetch("a", 9)
p h.except("a")
p h.slice("a", 1)
p h.values_at("a", 1)
p h.has_value?("x")
p h.has_value?(2.0)
p h.assoc("a")
p err { h.fetch("a") }
p err { h.fetch_values(1, :b) }
sh = { "a" => 1 }
p sh.assoc(2)
p sh.dig(:a)
p [1, 2].count("x")
p [1, 2].all?("x")
p [].all?("x")
p [1, 2].any?("x")
p [1, 2].none?("x")
p [1, 2].one?("x")
p [1, 2].delete("x")
p [1, 2].delete("x") { :none }
p ["a"].delete(1)
p [1, 2].index("x")
p [1, 2].rindex(:y)
p (1..2).include?("x")
p (1..2) === "x"
p (1..2).member?(nil)
p (1..2).eql?("x")
p (1..2).count("x")
p [1, 2.0].count(2)
p [1, 2].count(1.0)

p [1, 2].count(2**100)

class Pair
  def to_ary
    [1, 2]
  end
end

class Tbl
  def to_hash
    { "b" => 2 }
  end
end

class Inert
end
p [3, 4].zip(Pair.new)
p [3, 4].product(Pair.new)
p({ "a" => 1 }.merge(Tbl.new))
p err { [1].zip(5) }
p err { [1].zip(nil) }
p err { [1].zip(Inert.new) }
p err { [1].product("x") }
p err { { "a" => 1 }.merge(5) }
p err { { "a" => 1 }.merge(Inert.new) }
p({ "a" => 1 }[Sep.new])

def trace(label, value)
  puts label
  value
end
p trace("recv", [1]).product(trace("arg", [2]))

path = File.join(ZTMP, "spinel_typed_slot_conversion.txt")
File.write(path, "x")
p File.utime(nil, nil, path)
File.delete(path)
p Time.utc(2000, 5, nil)
p Time.utc(2000, 5, 6, nil, nil, nil).min
p err { Random.rand(nil) }
p err { 1.0.quo(nil) }
p err { 1.0.quo(:s) }
p err { 7.0.fdiv(Sep.new) }
p err { 7.fdiv("x") }
p "a1b2c".split(/\d/) { |x| p x }
p err { "a b".split(true) }

calls = 0
mk = -> { calls += 1; "k" }
p [h.fetch(mk.call, 0), h.dig(mk.call), h.values_at(mk.call), calls]

p err { sleep(true) }
p err { sleep(:s) }
p err { File.utime(false, false, "/nonexistent") }

p Regexp.escape(:"a.b")

class Needle
  def to_str
    puts "to_str asked"
    "b"
  end
end
misses = { "k" => "abcb" }
p err { misses["zz"].rindex(Needle.new) }
__END__
"a-b"
[nil, "a-b"]
"a---"
"---a"
"--a--"
1
3
1
true
true
["a", "b-c"]
["a", "b", "c"]
0
"a"
"-bc"
"-"
"-"
"\\-"
"-"
"-"
"v"
true
"w"
"w"
true
[TypeError, "no implicit conversion of Integer into String"]
[TypeError, "no implicit conversion of Integer into String"]
[TypeError, "no implicit conversion of Integer into String"]
[TypeError, "wrong argument type Integer (expected Regexp)"]
[TypeError, "no implicit conversion of Integer into String"]
[TypeError, "no implicit conversion of Integer into String"]
[TypeError, "no implicit conversion of Integer into String"]
[TypeError, "no implicit conversion of Integer into String"]
[[2], [1, 3]]
0
2
4
1
1
1
Integer
Integer
[[1], [2]]
3
[0, 1, [0, 1], [0, 1], ["a"]]
true
[TypeError, "no implicit conversion of String into Integer"]
[TypeError, "no implicit conversion of String into Integer"]
[TypeError, "no implicit conversion of String into Integer"]
[TypeError, "no implicit conversion of String into Integer"]
[TypeError, "no implicit conversion of String into Integer"]
[TypeError, "can't convert Array into an exact number"]
[TypeError, "can't convert String into time"]
[TypeError, "String can't be coerced into Float"]
[TypeError, "can't convert String into Float"]
[TypeError, "can't convert nil into Float"]
[TypeError, "can't convert Sep into Float"]
nil
nil
9
{1 => 2}
{1 => 2}
[nil, 2]
false
true
nil
[KeyError, "key not found: \"a\""]
[KeyError, "key not found: :b"]
nil
nil
0
false
true
false
true
false
nil
:none
nil
nil
nil
false
false
false
false
0
1
1
0
[[3, 1], [4, 2]]
[[3, 1], [3, 2], [4, 1], [4, 2]]
{"a" => 1, "b" => 2}
[TypeError, "wrong argument type Integer (must respond to :each)"]
[TypeError, "wrong argument type NilClass (must respond to :each)"]
[TypeError, "wrong argument type Inert (must respond to :each)"]
[TypeError, "no implicit conversion of String into Array"]
[TypeError, "no implicit conversion of Integer into Hash"]
[TypeError, "no implicit conversion of Inert into Hash"]
nil
recv
arg
[[1, 2]]
1
2000-05-01 00:00:00 UTC
0
[ArgumentError, "invalid argument - "]
[TypeError, "nil can't be coerced into Float"]
[TypeError, ":s can't be coerced into Float"]
[TypeError, "Sep can't be coerced into Float"]
[TypeError, "String can't be coerced into Integer"]
"a"
"b"
"c"
"a1b2c"
[TypeError, "wrong argument type true (expected Regexp)"]
[0, nil, [nil], 3]
[TypeError, "can't convert TrueClass into time interval"]
[TypeError, "can't convert Symbol into time interval"]
[TypeError, "can't convert FalseClass into time"]
"a\\.b"
[NoMethodError, "undefined method 'rindex' for nil"]
