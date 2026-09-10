# Array#sort_by performance across array sizes.
#
# sort_by previously lowered to a bubble sort that re-evaluated the key block on
# both operands of every comparison (O(n^2) block evaluations + O(n^2) sort). The
# Schwartzian lowering computes each key once (O(n) evaluations) and stable-sorts
# the indices with a merge sort (O(n log n)). This benchmark times sort_by at
# several sizes so the quadratic-vs-linearithmic gap is visible.
#
# The input is a deterministic multiplicative permutation (distinct keys, no
# ties), so the sorted output is byte-identical whether or not the sort is
# stable. The sorted checksum is printed to stdout (compared by the bench
# harness).

def perm_array(n)
  a = []
  i = 0
  while i < n
    a << (i * 1000003) % 2000003
    i += 1
  end
  a
end

def checksum(arr)
  s = 0
  i = 0
  while i < arr.length
    s = (s + arr[i] * (i + 1)) % 1000000007
    i += 1
  end
  s
end

[8000, 16000, 32000].each do |n|
  data = perm_array(n)
  sorted = data.sort_by { |x| x }
  puts "n=#{n} checksum=#{checksum(sorted)}"
end
puts "done"
__END__
n=8000 checksum=47828418
n=16000 checksum=191311428
n=32000 checksum=765203592
done
