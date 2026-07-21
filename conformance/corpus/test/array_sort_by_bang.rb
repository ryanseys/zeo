a = [3, 1, 2]
r = a.sort_by! { |x| -x }
p a
p r
s = %w[ccc a bb]
s.sort_by! { |w| w.length }
p s
f = [2.5, 0.5, 1.5]
f.sort_by! { |g| g }
p f
mixed = [3, "b", 1, "a"]
mixed.sort_by! { |m| m.to_s }
p mixed
e = []
p e.sort_by! { |q| q }
orig = [5, 1, 9]
ali = orig
ali.sort_by! { |z| z }
p orig
