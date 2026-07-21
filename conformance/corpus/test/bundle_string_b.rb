# Bundled tests:
#   - string_lines
#   - strings
#   - strings2
#   - strings3
#   - mutable_str

# === string_lines ===
def t_string_lines
  # Tests String#lines preserves trailing newline on each piece (CRuby semantics).
  p "hello\nworld\n".lines
  p "hello\nworld".lines
  p "single line".lines
  p "".lines
  p "\n\n\n".lines
  puts "hello\nworld\n".lines.length
  puts "hello\nworld\n".lines[0]
  puts "hello\nworld\n".lines[1]
end
t_string_lines

# === strings ===
def t_strings
  # Test Symbol and String methods
  
  # Symbol
  s = :hello
  puts s  # hello
  
  # String methods
  str = "Hello, World!"
  puts str.length      # 13
  puts str.upcase      # HELLO, WORLD!
  puts str.downcase    # hello, world!
  puts str.include?("World")  # true
  puts str.include?("xyz")    # false
  
  # String concatenation
  a = "foo"
  b = "bar"
  c = a + b
  puts c  # foobar
  
  # to_s on integers
  n = 42
  puts n.to_s  # 42
end
t_strings

# === strings2 ===
def t_strings2
  # Test additional String methods
  
  str = "  Hello, World!  "
  puts str.strip           # "Hello, World!"
  puts str.chomp           # "  Hello, World!  " (no trailing newline)
  
  s = "hello world"
  puts s.capitalize        # "Hello world"
  puts s.reverse           # "dlrow olleh"
  puts s.count("lo")       # 5
  puts s.start_with?("hel")  # true
  puts s.end_with?("rld")    # true
  puts s.gsub("l", "L")   # heLLo worLd
  puts s.sub("o", "0")    # hell0 world
  puts s.split(" ").length # 2
  
  # String repeat
  puts "ha" * 3            # hahaha
  
  # String comparison
  puts "abc" == "abc"      # true
  puts "abc" == "def"      # false
  puts "abc" < "def"       # true
end
t_strings2

# === strings3 ===
def t_strings3
  # Test additional string methods
  
  # ljust / rjust / center
  puts "hi".ljust(10)         # "hi        "
  puts "hi".rjust(10)         # "        hi"
  puts "hi".center(10)        # "    hi    "
  puts "hi".ljust(10, "*")    # "hi********"
  puts "hi".rjust(10, "0")    # "00000000hi"
  
  # lstrip / rstrip
  puts "  hello  ".lstrip     # "hello  "
  puts "  hello  ".rstrip     # "  hello"
  
  # tr / delete / squeeze
  puts "hello".tr("el", "ip")      # "hippo"
  puts "hello world".delete("lo")  # "he wrd"
  puts "aaabbbccc".squeeze          # "abc"
  
  # chars / bytes
  puts "abc".chars.length     # 3
  puts "abc".bytes.length     # 3
  
  # to_f
  puts ("3.14".to_f * 100).to_i  # 314
  
  # slice
  puts "hello"[1..3]          # "ell"
  puts "hello"[0, 2]          # "he"
  
  # hex / oct
  puts "ff".hex    # 255
  puts "77".oct    # 63
  
  # dup / freeze / frozen?
  s = "hello"
  puts s.dup       # "hello"
  puts s.freeze    # "hello"
  puts s.frozen?   # true
  
  # to_s
  puts "test".to_s  # "test"
  
  puts "done"
end
t_strings3

# === mutable_str ===
def t_mutable_str
  # Mutable strings (sp_String): the `<<` mutation path, replace/clear,
  # index access, gsub/sub, split, ljust/rjust, downcase/strip/...,
  # start_with?/end_with?/empty?/include?, to_s/to_i. Combines what
  # was previously split across mutable_str.rb / _2.rb / _3.rb.
  
  # << (true mutation, not reassignment)
  a = "hello"
  a << " world"
  a << "!"
  puts a          # hello world!
  puts a.length   # 12
  
  # Build string incrementally
  buf = ""
  5.times do |i|
    buf << i.to_s
    buf << ","
  end
  puts buf        # 0,1,2,3,4,
  
  # upcase / reverse / include? on mutable receiver
  b = "Hello World"
  puts b.upcase                 # HELLO WORLD
  puts b.reverse                # dlroW olleH
  puts b.include?("World")      # true
  
  # replace / clear
  c = "hello"
  c.replace("world")
  puts c          # world
  c.clear
  puts c.length   # 0
  
  # [] on mutable string
  d = "abcdef"
  d << "ghi"
  puts d[0]       # a
  puts d[-1]      # i
  puts d.length   # 9
  
  # gsub on mutable receiver
  e = "hello"
  e << " world"
  puts e.gsub("o", "0")  # hell0 w0rld
  
  # split on mutable receiver
  f = "a"
  f << ",b,c"
  parts = f.split(",")
  puts parts.length  # 3
  
  # + creates a new string, does not mutate the receiver
  g = "foo"
  g << "bar"
  h = g + "baz"
  puts g        # foobar (unchanged)
  puts h        # foobarbaz
  
  # to_s on mutable receiver
  tst = "test"
  tst << "ing"
  puts tst.to_s   # testing
  
  # downcase / strip / capitalize / start_with? / end_with? / empty?
  j = ""
  j << "Hello"
  j << " "
  j << "World"
  puts j.length             # 11
  puts j.downcase           # hello world
  puts j.strip              # Hello World
  puts j.capitalize         # Hello world
  puts j.start_with?("Hello")  # true
  puts j.end_with?("World")    # true
  puts j.empty?             # false
  
  # sub / gsub on mutable receiver
  puts j.gsub("l", "r")        # Herro Worrd
  puts j.sub("World", "Ruby")  # Hello Ruby
  
  # to_i via mutable receiver
  n = ""
  n << "42"
  puts n.to_i    # 42
  
  # ljust / rjust
  t = ""
  t << "hi"
  puts t.ljust(10)   # "hi        "
  puts t.rjust(10)   # "        hi"
  
  # include?
  puts j.include?("World")  # true
  puts j.include?("xyz")    # false
  
  puts "done"
end
t_mutable_str

