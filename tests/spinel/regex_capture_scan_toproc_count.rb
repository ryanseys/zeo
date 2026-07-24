# s[/re/, n] and s[/re/, :name] captures, the String#scan block form,
# count on a boxed-array block parameter, explicit Symbol#to_proc, and
# combination/permutation over non-integer arrays.
p "2024-01-15"[/(\d+)-(\d+)/, 2]
p "hello world"[/(\w+) (\w+)/, 1]
p "2024-01"[/(?<y>\d+)-(?<m>\d+)/, :y]
p "abc"[/(x)(y)/, 1]
p "hello"[/l+/, 0]
result = "hello world".scan(/\w+/) { |w| print w.upcase, " " }
puts
p result
p [[1, 2], [3, 4, 5]].map { |a| a.count }
p [[1, 2], [3, 4, 5]].map(&:count)
p [[1, 2, 2], [3]].map { |a| a.count(2) }
p [[1, 2, 3], [4]].map { |a| a.count { |x| x > 1 } }
p [[1, 2, 3]].sum { |a| a.count }
up = :upcase.to_proc
p up.call("hi")
add = :+.to_proc
p add.call(2, 3)
p ["a", "b", "c"].combination(2).to_a
p [:a, :b, :c].combination(2).to_a
p [1.0, 2.0, 3.0].combination(2).to_a
p ["a", "b", "c"].permutation(2).to_a
p ["a", "b"].repeated_combination(2).to_a
p "ABC".scan(/[a-z]/i)
p "Hello World!".gsub(/[^a-z]/i, "")
p("ABC" =~ /[a-z]/i)
p "aBc".scan(/[A-Z]/i)
p "x1y2".scan(/[a-m0-5]/i)
