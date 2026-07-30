# A `begin ... end` with no rescue / else / ensure protects nothing, so it
# emits no handler frame -- the do-while idiom above all. The forms that DO
# need one still behave.
i = 0
begin
  i += 1
end while i < 3
p i

j = 10
begin
  j -= 1
end until j <= 7
p j

v = begin
  1 + 1
end
p v

w = begin
  "a"
  "b"
end
p w

r = begin
  raise "x"
rescue => e
  e.message
end
p r

def f
  begin
    return 1
  ensure
    puts "ens"
  end
end
p f

def g
  begin
    begin
      raise "inner"
    end
  rescue => e
    "caught #{e.message}"
  end
end
p g

k = 0
begin
  k += 1
  next if k == 1
end while k < 2
p k

n = 0
loop do
  begin
    n += 1
  end
  break if n > 2
end
p n
