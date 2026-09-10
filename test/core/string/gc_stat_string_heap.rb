# GC.stat surfaces the string heap as str_bytes and str_count. String data
# lives on a heap of its own, kept out of the object-heap `bytes` figure, so a
# string-heavy workload can hold gigabytes that `bytes` never reflects.
# Retaining 1000 strings makes str_count >= 1000 and str_bytes > 0.
arr = []
i = 0
while i < 1000
  arr.push(i.to_s)
  i += 1
end
g = GC.stat
puts "str_count >= 1000: " + (g["str_count"] >= 1000 ? "yes" : "no")
puts "str_bytes > 0: " + (g["str_bytes"] > 0 ? "yes" : "no")
puts "retained: " + arr.length.to_s
__END__
#@ stderr
core/string/gc_stat_string_heap.rb:12:in '<main>': undefined method '>=' for nil (NoMethodError)
#@ exit 1
