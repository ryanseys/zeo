# A jump out of an ensure-carrying `begin` runs the ensure first: it
# travels as a signal and the `begin` settles it onto its own target.
i = 0
while i < 4
  begin
    break if i == 2
    puts "b#{i}"
  ensure
    puts "e#{i}"
  end
  i += 1
end
puts "---"
j = 0
while j < 4
  j += 1
  begin
    next if j == 2
    puts "n#{j}"
  ensure
    puts "ne#{j}"
  end
end
puts "---"
k = 0
tries = 0
while k < 2
  begin
    tries += 1
    redo if tries == 1
    puts "k#{k}"
  ensure
    puts "ke#{k}"
  end
  k += 1
end
puts "---"
m = 0
while m < 3
  m += 1
  begin
    raise "x" if m == 1
    puts "m#{m}"
  rescue
    next
  ensure
    puts "me#{m}"
  end
end
puts "---"
n = 0
while n < 3
  n += 1
  begin
    puts "n#{n}"
  ensure
    break if n == 2
  end
end
puts "done"
puts "==="; 
for x in [1,2,3]
  begin
    break if x == 3
    next if x == 2
    puts "f#{x}"
  ensure
    puts "fe#{x}"
  end
end
puts "---"
3.times do |i|
  begin
    next if i == 1
    puts "t#{i}"
  ensure
    puts "te#{i}"
  end
end
puts "---"
[1,2,3].each do |i|
  begin
    break if i == 3
    puts "a#{i}"
  ensure
    puts "ae#{i}"
  end
end
puts "---"
y = 0
until y >= 3
  y += 1
  begin
    begin
      next if y == 2
      puts "u#{y}"
    ensure
      puts "ui#{y}"
    end
  ensure
    puts "uo#{y}"
  end
end
puts "---"
z = 0
while z < 3
  z += 1
  begin
    begin
      break if z == 2
      puts "w#{z}"
    ensure
      puts "wi#{z}"
    end
  rescue
    puts "never"
  ensure
    puts "wo#{z}"
  end
end
puts "done"
puts "==="; 
def a
  begin
    return 1
  ensure
    puts "ensure a"
  end
end
p a

def b
  begin
    raise "boom"
  rescue => e
    return "r:#{e.message}"
  ensure
    puts "ensure b"
  end
end
p b

def c
  [10, 20].each do |x|
    begin
      return x if x == 20
      puts "seen #{x}"
    ensure
      puts "ensure c #{x}"
    end
  end
  :none
end
p c

def d
  begin
    begin
      return 5
    ensure
      puts "inner d"
    end
  ensure
    puts "outer d"
  end
end
p d

def e
  begin
    return 1
  ensure
    return 2
  end
end
p e

def f
  begin
    yield
    return :after
  ensure
    puts "ensure f"
  end
end
p(f { 1 })
