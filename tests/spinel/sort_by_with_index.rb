# A blockless sort_by answers an Enumerator whose with_index feeds the index to
# the key block. There was no Enumerator arm for it, so the chain raised
# NoMethodError; the equivalent spelling over pairs serves it.
a = ["b", "aa", "a"]
p a.sort_by.with_index { |w, i| [-w.length, i] }
p a.sort_by.with_index { |w, i| i }
p a.sort_by.with_index { |w, i| -i }

n = [3, 1, 2]
p n.sort_by.with_index { |v, i| v }
p n.sort_by.with_index { |v, i| [v % 2, i] }

# a single element, and one where the key ties across every element
p ["only"].sort_by.with_index { |w, i| i }
p [5, 5, 5].sort_by.with_index { |v, i| 0 }

# through a local, and over a computed receiver
words = %w[delta a bb ccc]
p words.sort_by.with_index { |w, i| [w.length, i] }
p (words + ["x"]).sort_by.with_index { |w, i| w }

# the plain forms still work
p a.sort_by { |w| w.length }
p a.each.with_index.to_a
p a.map.with_index { |w, i| "#{w}#{i}" }
p a.select.with_index { |w, i| i > 0 }
