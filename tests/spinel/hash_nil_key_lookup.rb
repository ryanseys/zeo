NAMED = { "navy" => "#000080", "red" => "#ff0000" }

def valid?(color)
  NAMED.key?(color)
end

p valid?("navy")
p valid?(nil)
p NAMED[nil]
p NAMED.fetch(nil, "none")
p NAMED.include?(nil)
p NAMED.member?(nil)
p NAMED.size

counts = Hash.new(0)
counts["a"] += 1
p counts[nil]
p counts.key?(nil)

mixed = { "k" => 1 }
p mixed.key?(nil)
p mixed.size

vals = { "k" => [1, 2] }
p vals[nil]
p vals.key?(nil)
