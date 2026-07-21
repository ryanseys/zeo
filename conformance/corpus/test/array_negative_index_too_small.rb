# Issue #839: an extreme negative index on []= raises IndexError
# per MRI. Spinel previously silently no-op'd when the index after
# `+= len` was still negative.
a = [1, 2, 3]
begin
  a[-999] = 99
rescue IndexError => e
  puts "int: " + e.message
end
puts a.inspect

b = ["a", "b", "c"]
begin
  b[-50] = "x"
rescue IndexError => e
  puts "str: " + e.message
end
puts b.inspect

# Valid negative index still works.
a[-1] = 33
puts a.inspect
