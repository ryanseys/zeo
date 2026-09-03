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
puts "===";
r = [1,2,3].map do |x|
  begin
    next x * 10 if x == 2
    x
  ensure
    puts "ens #{x}"
  end
end
p r
r2 = [1,2,3].each do |x|
  begin
    break :stop if x == 2
  ensure
    puts "b ens #{x}"
  end
end
p r2
tries = 0
r3 = [1,2].map do |x|
  begin
    tries += 1
    redo if x == 1 && tries == 1
    x
  ensure
    puts "r ens #{x} #{tries}"
  end
end
p r3
def deep
  [1,2].each do |x|
    begin
      begin
        next if x == 1
        return :from_block
      ensure
        puts "inner #{x}"
      end
    ensure
      puts "outer #{x}"
    end
  end
  :none
end
p deep
r = [1,2,3].map do |x|
  begin
    next x * 10 if x == 2
    x
  ensure
    puts "ens #{x}"
  end
end
p r
r2 = [1,2,3].each do |x|
  begin
    break :stop if x == 2
  ensure
    puts "b ens #{x}"
  end
end
p r2
__END__
b0
e0
b1
e1
e2
---
n1
ne1
ne2
n3
ne3
n4
ne4
---
ke0
k0
ke0
k1
ke1
---
me1
m2
me2
m3
me3
---
n1
n2
done
===
f1
fe1
fe2
fe3
---
t0
te0
te1
t2
te2
---
a1
ae1
a2
ae2
ae3
---
u1
ui1
uo1
ui2
uo2
u3
ui3
uo3
---
w1
wi1
wo1
wi2
wo2
done
===
ensure a
1
ensure b
"r:boom"
seen 10
ensure c 10
ensure c 20
20
inner d
outer d
5
2
ensure f
:after
===
ens 1
ens 2
ens 3
[1, 20, 3]
b ens 1
b ens 2
:stop
r ens 1 1
r ens 1 2
r ens 2 3
[1, 2]
inner 1
outer 1
inner 2
outer 2
:from_block
ens 1
ens 2
ens 3
[1, 20, 3]
b ens 1
b ens 2
:stop
