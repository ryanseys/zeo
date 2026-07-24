# GC roots for runtime array builders and their string arguments. Each of
# these allocates per element while building (or while walking a pattern
# that is itself a fresh unrooted interpolation temp), so enough entries
# trigger a collection MID-BUILD: the result array, its pushed names, or
# the argument string got swept under the walk (spin test crashed with
# double-free/SIGSEGV once test/ held ~50 files).
dir = "gc_glob_probe.tmp"
Dir.mkdir(dir) unless Dir.exist?(dir)
80.times { |i| File.write("#{dir}/t#{i}.rb", "puts 1\n") }

# Dir.glob through a fresh interpolated pattern (the #2178 shape)
names = Dir.glob("#{dir}/*.rb")
puts names.length
puts names.include?("#{dir}/t79.rb")

# Dir.entries on the same tree
puts Dir.entries(dir).length

80.times { |i| File.delete("#{dir}/t#{i}.rb") }
Dir.rmdir(dir)

# scan: many matches from a fresh temp receiver
puts ("ab " * 500).scan(/a./).length
puts ("x1 y2 " * 300).scan(/([a-z])(\d)/).length

# rpartition via regexp on a fresh temp
p ("k-" * 200 + "end").rpartition(/k-/)[2]

# combination/repeated_combination allocate one array per emitted row
puts (1..9).to_a.combination(4).to_a.length
puts [1, 2, 3, 4].repeated_combination(4).to_a.length
