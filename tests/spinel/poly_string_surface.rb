# The String surface on a value that arrives boxed: a Fiber#resume result, a
# Queue#pop, a Thread#value. Every one of these answers on a plain String
# local, so each line that disagrees is a missing or wrong arm on the boxed
# path rather than a String bug.
def fv(s)
  f = Fiber.new { Fiber.yield(s); nil }
  f.resume
end

v = fv("aabbcc")

# --- reported missing (#3445) ---
r = (v.squeeze rescue $!.class); p r
r = (v.index("b") rescue $!.class); p r
r = (v.rindex("b") rescue $!.class); p r
r = (v.byteindex("b") rescue $!.class); p r
r = (v.partition("b") rescue $!.class); p r
r = (v.rpartition("b") rescue $!.class); p r
r = (v.slice(1, 2) rescue $!.class); p r
r = (v.hex rescue $!.class); p r
r = (v.oct rescue $!.class); p r
r = (v.tr_s("ab", "x") rescue $!.class); p r
r = (v.casecmp("AABBCC") rescue $!.class); p r
r = (v.casecmp?("AABBCC") rescue $!.class); p r
r = (fv("abc").crypt("ab") rescue $!.class); p r

# the value-form mutators, which answer the new contents and update an lvalue
m = fv(String.new("aabbcc"))
r = (m.insert(1, "X") rescue $!.class); p r
p m
m2 = fv(String.new("aabbcc"))
r = (m2.squeeze! rescue $!.class); p r
p m2
m3 = fv(String.new("abc"))
r = (m3.squeeze! rescue $!.class); p r   # no change -> nil
p m3
m4 = fv(String.new(" pad "))
r = (m4.strip! rescue $!.class); p r
r = (m4.upcase! rescue $!.class); p r
p m4

# --- reported wrong, not missing (#3446) ---
r = (v.count("a") rescue $!.class); p r
r = (v.sum rescue $!.class); p r
r = (v.sum(8) rescue $!.class); p r

# --- reported wrong (#3447) ---
f2 = fv("v=%d")
r = (f2 % [5] rescue $!.class); p r
r = (f2 % 5 rescue $!.class); p r

# --- already working: these must keep working ---
p v
p v[1]
p v[1, 2]
p v.length
p v.ord
p v.delete("a")
p v.upcase
p v.split("b")
p v.sub("aa", "x")
p v.gsub("b", "y")
p v.tr("a", "z")
p v.squeeze("abc")
p v.delete_prefix("aa")
p v.delete_suffix("cc")
p v.chars
p v.bytes
p v.start_with?("aa")
p v.include?("bb")
p v.reverse
p v.center(10, "-")
p v.to_sym
p v.each_char.to_a
p v.each_byte.sum

# the same value out of a Queue and a Thread
q = Queue.new
q.push("aabbcc")
qv = q.pop
r = (qv.index("b") rescue $!.class); p r
r = (qv.count("a") rescue $!.class); p r

t = Thread.new { "aabbcc" }
tv = t.value
r = (tv.partition("b") rescue $!.class); p r
r = (tv.sum rescue $!.class); p r

# A boxed Array answers the names it shares with String on its own terms: the
# String arms must never claim a receiver whose runtime class is unknown.
a = fv([1, 2, 2, 3])
p a.count(2)
p a.index(2)
p a.rindex(2)
p a.sum
p a.slice(1, 2)
p a.insert(1, 9)
p a
p fv({ "a" => 1, "b" => 2 }).count
