# Huffman encoding benchmark
# Counts byte frequencies in a generated text, builds a Huffman tree,
# generates per-byte codes, and outputs the total encoded bit length.
# Exercises: nullable self-referential objects (tree), int-indexed arrays,
# recursive tree walk, and hash-of-codes via fixed-size lookup table.

class Node
  attr_accessor :ch, :freq, :left, :right
  def initialize(ch, freq)
    @ch = ch
    @freq = freq
    @left = nil
    @right = nil
  end
end

# Generate a deterministic ~N-byte text from a small alphabet.
def gen_text(n)
  letters = "abcdefghijklmnopqrstuvwxyz ,.?!"
  out = ""
  seed = 12345
  i = 0
  while i < n
    seed = (seed * 1103515245 + 12345) % 2147483648
    idx = seed % letters.length
    out << letters[idx]
    i = i + 1
  end
  out
end

# Recursive walk: assign code paths to leaves into `codes` (byte -> string).
def walk(node, path, codes)
  if node.left == nil && node.right == nil
    codes[node.ch] = path
    return
  end
  if node.left != nil
    walk(node.left, path + "0", codes)
  end
  if node.right != nil
    walk(node.right, path + "1", codes)
  end
end

n = Integer(ARGV[0] || 200000)
text = gen_text(n)
bytes = text.bytes

# Byte frequencies via 256-entry table (alphabet fits in ASCII).
freq = []
i = 0
while i < 256
  freq.push(0)
  i = i + 1
end
i = 0
while i < bytes.length
  bv = bytes[i]
  freq[bv] = freq[bv] + 1
  i = i + 1
end

# Seed the priority queue with one leaf per byte seen.
queue = []
i = 0
while i < 256
  if freq[i] > 0
    queue.push(Node.new(i, freq[i]))
  end
  i = i + 1
end

# Build the tree by repeatedly merging the two smallest-freq nodes.
# Linear-search for the two minima — small alphabet keeps this cheap.
while queue.length > 1
  min1 = 0
  i = 1
  while i < queue.length
    if queue[i].freq < queue[min1].freq
      min1 = i
    end
    i = i + 1
  end
  min2 = -1
  i = 0
  while i < queue.length
    if i != min1
      if min2 < 0 || queue[i].freq < queue[min2].freq
        min2 = i
      end
    end
    i = i + 1
  end
  a = queue[min1]
  b = queue[min2]
  new_queue = []
  i = 0
  while i < queue.length
    if i != min1 && i != min2
      new_queue.push(queue[i])
    end
    i = i + 1
  end
  parent = Node.new(0, a.freq + b.freq)
  parent.left = a
  parent.right = b
  new_queue.push(parent)
  queue = new_queue
end
root = queue[0]

# Code table: 256 slots, one string each.
codes = []
i = 0
while i < 256
  codes.push("")
  i = i + 1
end
walk(root, "", codes)

# Sum the encoded bit length over the whole text.
total_bits = 0
i = 0
while i < bytes.length
  total_bits = total_bits + codes[bytes[i]].length
  i = i + 1
end

puts total_bits

# Count alphabet size (chars actually used).
alphabet = 0
i = 0
while i < 256
  if freq[i] > 0
    alphabet = alphabet + 1
  end
  i = i + 1
end
puts alphabet
