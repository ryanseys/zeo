# ENV snapshot GC safety (#2842), grep with reference/module patterns
# (#2843), hash/ENV lazy chains (#2845), ENV mutator chaining and the
# bang trio (#2844), and slice_before/after === patterns (#2847).
300.times { |i| ENV["ZZBIG_#{i}"] = "v" * 200 }
20.times do
  n = 0
  ENV.each_value { |v| n += 1 }
  raise "bad" unless n > 300
end
puts "ok"
p [[1, 2], [3, 4]].grep(Array).length
p [1, [2], {3 => 4}, "x"].grep(Array)
p [1, [2], {3 => 4}, "x"].grep(Hash)
p [1, [2], "x"].grep(Object).length
p [1, 2.5, "x", :s].grep(Comparable)
p [1, [2], "x"].grep(Enumerable)
p [1, 2, 3].grep(Array)
p [1, "a", 2].grep_v(String)
class UU2; end
p [UU2.new, 1].grep(UU2).length
h = {"a" => 1}
p ENV.grep(Array).length >= 0 rescue p :env_grep_fail
p({ a: 1, b: 2 }.lazy.select { |k, v| v > 0 }.first(1))
h = { "x" => 1, "y" => 2 }
p h.lazy.map { |k, v| k }.first(2)
ENV['ZZ_L'] = '9'
p ENV.lazy.select { |k, v| k == 'ZZ_L' }.first(1)
a = [["x", 1], ["y", 2]]
p a.lazy.map { |k, v| k }.first(2)
p [1, 2, 3, 1, 2].slice_before(2..3).to_a
p %w[a bb c].slice_before(/b/).to_a
p [1, 2, 3].slice_after(Integer).to_a
p [1, "x", 2, "y"].slice_before(String).to_a
p [1, 2, 3, 1, 2].slice_after(2..3).to_a
p [1, 2, 3, 1].slice_before(2).to_a
p [1, 2.5, 3].slice_before(2.0..3.0).to_a
ENV.delete('ZZ_A'); ENV.delete('ZZ_B')
ENV.update('ZZ_A' => '1').update('ZZ_B' => '2')
p ENV['ZZ_A']
p ENV['ZZ_B']
p ENV.reject! { |k, v| false }
p ENV.class
