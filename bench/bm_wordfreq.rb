# Word frequency counter using str_int_hash
# Tests Hash, String, Array operations

def gen_words(n)
  words = ["the", "of", "and", "to", "in", "a", "is", "that", "for", "it",
           "was", "on", "are", "be", "with", "as", "at", "by", "this", "from"]
  result = []
  seed = 42
  i = 0
  while i < n
    seed = (seed * 1103515245 + 12345) % 2147483648
    idx = seed / 107374182
    result.push(words[idx])
    i = i + 1
  end
  result
end

n = Integer(ARGV[0] || 10000)
words = gen_words(n)

freq = {}
i = 0
while i < words.length
  w = words[i]
  if freq.key?(w)
    freq[w] = freq[w] + 1
  else
    freq[w] = 1
  end
  i = i + 1
end

# Sum all counts
keys = freq.keys
total = 0
i = 0
while i < keys.length
  total = total + freq[keys[i]]
  i = i + 1
end
puts total
puts keys.length
