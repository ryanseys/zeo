# A method defined in a Struct.new / Data.define block overrides the built-in
# member accessor of the same name, as it does in CRuby. `[]` written in the
# block was ignored: the call went to the generated member lookup, so a key
# that is not a member raised
#
#   NameError: no member 'text' in struct
#
# instead of running the user's body. The same applies to any built-in the
# block redefines (`to_s`, `==`, ...).
#
# Found porting tobi/try, which wraps a Hash payload in a Data and defines
# `[]` to forward unknown keys into that Hash.
Item = Data.define(:data, :score) do
  def [](key)
    if key == :score
      score
    else
      data[key]
    end
  end
end

i = Item.new(data: { text: "hello", path: "/tmp/x" }, score: 1.5)
p i[:score]
p i[:text]
p i[:path]

# The generated readers still work alongside the override.
p i.score
p i.data[:text]

# Struct.new takes the same treatment.
Pair = Struct.new(:a, :b) do
  def [](key)
    "custom:#{key}"
  end
end
s = Pair.new(1, 2)
p s[:a]
p s[:zzz]
p s.a
p s.b

# A Struct that does NOT override [] keeps the built-in member lookup,
# including the integer-offset form and the NameError for an unknown name.
Plain = Struct.new(:x, :y)
pl = Plain.new(7, 8)
p pl[:x]
p pl[1]
p pl[-1]
begin
  pl[:nope]
rescue NameError => e
  puts "NameError: #{e.message}"
end
