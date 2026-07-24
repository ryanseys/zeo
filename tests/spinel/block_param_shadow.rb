# Block parameters shadow outer differently-typed locals correctly.
# Without push_scope/declare_var around the block body compile, infer_type
# returned the outer scope's type for the block param, dispatching the
# wrong runtime call (e.g. sp_str_repeat for an int param).

# --- each on int_array, outer string v (the original report) ---
def f1
  v = "hello"
  [1, 2, 3].each do |v|
    puts v * 10
  end
  puts v
end
f1
# 10
# 20
# 30
# hello

# --- each_with_index, outer string i ---
def f2
  i = "outer"
  ["a", "b"].each_with_index do |s, i|
    puts s + i.to_s
  end
  puts i
end
f2
# a0
# b1
# outer

# --- each_with_index on str_array, outer int i ---
def f3
  i = 99
  ["a", "b", "c"].each_with_index do |s, i|
    puts s + ":" + i.to_s
  end
  puts i
end
f3
# a:0
# b:1
# c:2
# 99

# --- map on int_array, outer string i ---
def f4
  i = "outer"
  out = [10, 20, 30].map { |i| i + 1 }
  puts out[0]               # 11
  puts out[1]               # 21
  puts out[2]               # 31
  puts i                    # outer
end
f4

# --- map on str_array, outer int s ---
def f5
  s = 42
  words = ["a", "b", "c"]
  up = words.map { |s| s.upcase }
  puts up[0]                # A
  puts up[1]                # B
  puts up[2]                # C
  puts s                    # 42
end
f5

# --- select on int_array, outer string n ---
def f6
  n = "kept"
  out = [1, 2, 3, 4].select { |n| n > 2 }
  puts out.length           # 2
  puts out[0]               # 3
  puts out[1]               # 4
  puts n                    # kept
end
f6

# --- reject on int_array, outer string n ---
def f7
  n = "drop"
  out = [1, 2, 3, 4].reject { |n| n > 2 }
  puts out.length           # 2
  puts out[0]               # 1
  puts out[1]               # 2
  puts n                    # drop
end
f7

# --- partition on int_array, outer string n ---
def f8
  n = "outer"
  parts = [1, 2, 3, 4, 5].partition { |n| n > 2 }
  puts parts[0].length      # 3
  puts parts[0][0]          # 3
  puts parts[0][1]          # 4
  puts parts[0][2]          # 5
  puts parts[1].length      # 2
  puts parts[1][0]          # 1
  puts parts[1][1]          # 2
  puts n                    # outer
end
f8

# --- partition on str_array, outer int x ---
def f9
  x = 99
  parts = ["a", "bb", "ccc"].partition { |x| x.length > 1 }
  puts parts[0].length      # 2
  puts parts[0][0]          # bb
  puts parts[0][1]          # ccc
  puts parts[1].length      # 1
  puts parts[1][0]          # a
  puts x                    # 99
end
f9

# --- each_char on string, outer int c ---
def f10
  c = 7
  "abc".each_char do |c|
    puts c + "!"
  end
  puts c
end
f10
# a!
# b!
# c!
# 7

# --- each_byte on string, outer string b ---
def f11
  b = "kept"
  "AB".each_byte do |b|
    puts b + 1
  end
  puts b
end
f11
# 66
# 67
# kept

# --- cycle on int_array with count, outer string x ---
def f12
  x = "outer"
  [1, 2].cycle(2) do |x|
    puts x * 10
  end
  puts x
end
f12
# 10
# 20
# 10
# 20
# outer

# --- scan with block on string, outer int m ---
def f13
  m = 99
  "a1b2c3".scan(/\d+/) do |m|
    puts m + "!"
  end
  puts m
end
f13
# 1!
# 2!
# 3!
# 99

# --- each_slice on int_array, outer string v ---
def f14
  v = "hello"
  [1, 2, 3, 4].each_slice(2) do |v|
    puts v.length
  end
  puts v
end
f14
# 2
# 2
# hello

# --- each_cons on str_array, outer int c ---
def f15
  c = 7
  ["a", "b", "c", "d"].each_cons(3) do |c|
    puts c.length
  end
  puts c
end
f15
# 3
# 3
# 7

# --- tap on int receiver, outer string v ---
def f16
  v = "hello"
  42.tap do |v|
    puts v + 1
  end
  puts v
end
f16
# 43
# hello

# --- tap on string receiver, outer int s ---
def f17
  s = 99
  "abc".tap do |s|
    puts s + "!"
  end
  puts s
end
f17
# abc!
# 99

# --- then on int receiver, outer string v ---
def f18
  v = "hello"
  out = 42.then { |v| v + 1 }
  puts out
  puts v
end
f18
# 43
# hello

# --- yield_self with int receiver returning string, outer float v ---
def f19
  v = 3.14
  out = 42.yield_self { |v| v.to_s + "!" }
  puts out
  puts v
end
f19
# 42!
# 3.14
