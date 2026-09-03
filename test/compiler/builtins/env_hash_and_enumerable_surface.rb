# `ENV`'s own singleton rows and the Enumerable half its singleton class
# includes. Both matter for dispatch: ENV's class IS `Object`, so a row it
# does not declare falls through to `Object`/`Kernel` -- which is how
# `ENV.select` once reached the private `Kernel#select`.
ENV.keys.each { |k| ENV.delete(k) unless k == "PATH" }
ENV["ZEO_A"] = "1"
ENV["ZEO_B"] = "2"

p ENV.select { |k, v| k.start_with?("ZEO_") }
p ENV.filter { |k, v| k.start_with?("ZEO_") }
p ENV.reject { |k, v| !k.start_with?("ZEO_") }
p ENV.select { |k, v| false }
p ENV.select.class, ENV.each_key.class, ENV.each_value.class
p ENV.to_a.class, ENV.to_a.include?(["ZEO_A", "1"])
p ENV.invert["1"], ENV.rassoc("1")
p ENV.has_value?("1"), ENV.value?("2"), ENV.value?("nope-zzz")
p ENV.except("PATH", "ZEO_B")
p ENV.rehash

ks = []
ENV.each_key { |k| ks << k }
p ks.include?("ZEO_A")
vs = []
ENV.each_value { |v| vs << v }
p vs.include?("2")

# Every iterator answers ENV itself, and IDENTITY survives -- a Hash snapshot
# standing in for ENV would fail both.
p ENV.equal?(ENV)
p (ENV.each_key { |k| }).equal?(ENV)
p (ENV.each { |k, v| }).equal?(ENV)
p ENV.object_id == ENV.object_id
p ENV.class, ENV.to_s, ENV.frozen?, ENV.nil?

# The Enumerable half, served by a snapshot.
p ENV.count > 0
p ENV.map { |k, v| k }.class, ENV.min.class, ENV.sort.class
p ENV.find { |k, v| k == "ZEO_A" }
p ENV.each_with_index.to_a.first.class
p ENV.tally.class, ENV.grep(/x/).class, ENV.lazy.class
p ENV.include?("ZEO_A"), ENV.any? { |k, v| k == "ZEO_A" }, ENV.length
__END__
{"ZEO_A" => "1", "ZEO_B" => "2"}
{"ZEO_A" => "1", "ZEO_B" => "2"}
{"ZEO_A" => "1", "ZEO_B" => "2"}
{}
Enumerator
Enumerator
Enumerator
Array
true
"ZEO_A"
["ZEO_A", "1"]
true
true
false
{"ZEO_A" => "1"}
nil
true
true
true
true
true
true
Object
"ENV"
false
false
true
Array
Array
Array
["ZEO_A", "1"]
Array
Hash
Array
Enumerator::Lazy
true
true
3
