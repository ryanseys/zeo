# lrama-required features. Combines what was previously split across
# lrama_features.rb / _2.rb / _3.rb (the sections were "part 1/2/3"
# of the same coverage matrix).

# === Part 1 ===

# private (should be ignored)
class Foo
  def pub; "public"; end
  private
  def priv; "private"; end
end
f = Foo.new
puts f.pub

# bare case (case without expression)
x = 5
result_x = case
           when x > 10 then "big"
           when x > 3 then "medium"
           else "small"
           end
puts result_x

# Array.new(n, val)
arr_new = Array.new(5, 42)
puts arr_new.length  # 5
puts arr_new[0]      # 42
puts arr_new[4]      # 42

# Array#compact (no-op for IntArray)
a2 = [1, 2, 3]
puts a2.compact.length  # 3

# Array#flatten (no-op for IntArray)
puts a2.flatten.length  # 3

# Array#unshift
a3 = [2, 3, 4]
a3.unshift(1)
puts a3[0]  # 1
puts a3.length  # 4

# Array#reverse
a4 = [1, 2, 3]
rev = a4.reverse
puts rev[0]  # 3
puts rev[2]  # 1

# Float::INFINITY
puts Float::INFINITY > 999999  # true

puts "done"

# === Part 2 ===

# A9: Hash#merge (sp_StrIntHash)
mrg1 = {"a" => 1, "b" => 2}
mrg2 = {"b" => 3, "c" => 4}
mrg3 = mrg1.merge(mrg2)
puts mrg3["a"]  # 1
puts mrg3["b"]  # 3  (mrg2 overrides mrg1)
puts mrg3["c"]  # 4

# A19: Array#dup
arr_dup = [10, 20, 30]
arr_dup2 = arr_dup.dup
puts arr_dup2[0]  # 10
puts arr_dup2.length  # 3

# A19: String#dup (const string)
sd1 = "hello"
sd2 = sd1.dup
puts sd2  # hello

# A20: Hash.new(0) with default value
counter = Hash.new(0)
counter["x"] = counter["x"] + 1
counter["x"] = counter["x"] + 1
counter["y"] = counter["y"] + 1
puts counter["x"]  # 2
puts counter["y"]  # 1
puts counter["z"]  # 0 (default)

# B2: attr_writer
class Writer
  attr_reader :name
  attr_writer :name
  def initialize(n)
    @name = n
  end
end
w = Writer.new("before")
puts w.name  # before
w.name = "after"
puts w.name  # after

# B3: Comparable (include Comparable + def <=>)
class Weight
  include Comparable
  attr_reader :grams
  def initialize(g)
    @grams = g
  end
  def <=>(other)
    @grams - other.grams
  end
end
w1 = Weight.new(100)
w2 = Weight.new(200)
w3 = Weight.new(100)
puts w1 < w2    # true
puts w2 > w1    # true
puts w1 == w3   # true
puts w1 > w2    # false
puts w1 <= w3   # true
puts w2 >= w1   # true

puts "done"

# === Part 3 ===

# A10: Hash#transform_values (sp_StrIntHash)
tv = {"a" => 1, "b" => 2, "c" => 3}
tv2 = tv.transform_values { |v| v * 10 }
puts tv2["a"]  # 10
puts tv2["b"]  # 20
puts tv2["c"]  # 30
puts tv["a"]   # 1  (original unchanged)

# A18: Array#zip
za = [1, 2, 3]
zb = [4, 5, 6]
zipped = za.zip(zb)
puts zipped.length  # 3

# Zip with different lengths
zc = [10, 20]
zipped2 = za.zip(zc)
puts zipped2.length  # 3

puts "done"
