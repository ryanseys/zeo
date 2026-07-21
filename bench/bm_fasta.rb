# The Computer Language Benchmarks Game - Fasta
# Generate random DNA sequences

IM = 139968
IA = 3877
IC = 29573

$last = 42

def gen_random(max)
  $last = ($last * IA + IC) % IM
  max * $last / IM
end

def make_repeat_fasta(id, desc, src, n)
  puts ">" + id + " " + desc
  k = 0
  i = 0
  while i < n
    len = 60
    if n - i < 60
      len = n - i
    end
    line = ""
    j = 0
    while j < len
      line = line + src[k % src.length]
      k = k + 1
      j = j + 1
    end
    puts line
    i = i + len
  end
end

def make_random_fasta(id, desc, table_chars, table_probs, n)
  puts ">" + id + " " + desc
  # Build cumulative probabilities
  cp = Array.new(table_probs.length, 0.0)
  sum = 0.0
  k = 0
  while k < table_probs.length
    sum = sum + table_probs[k]
    cp[k] = sum
    k = k + 1
  end
  i = 0
  while i < n
    len = 60
    if n - i < 60
      len = n - i
    end
    line = ""
    j = 0
    while j < len
      r = gen_random(1.0)
      # Linear search
      c = 0
      while c < cp.length - 1
        if r < cp[c]
          break
        end
        c = c + 1
      end
      line = line + table_chars[c]
      j = j + 1
    end
    puts line
    i = i + len
  end
end

ALU = "GGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACCTGAGGTCAGGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAATACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCCAGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAAAA"

n = Integer(ARGV[0] || 1000)

make_repeat_fasta("ONE", "Homo sapiens alu", ALU, n * 2)

iub_chars = ["a", "c", "g", "t", "B", "D", "H", "K", "M", "N", "R", "S", "V", "W", "Y"]
iub_probs = [0.27, 0.12, 0.12, 0.27, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02, 0.02]

make_random_fasta("TWO", "IUB ambiguity codes", iub_chars, iub_probs, n * 3)

homo_chars = ["a", "c", "g", "t"]
homo_probs = [0.3029549426680, 0.1979883004921, 0.1975473066391, 0.3015094502008]

make_random_fasta("THREE", "Homo sapiens frequency", homo_chars, homo_probs, n * 5)
