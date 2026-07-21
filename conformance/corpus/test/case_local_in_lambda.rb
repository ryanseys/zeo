# A local assigned inside a `case`/`when` branch of a lambda/proc
# body must be declared in the proc's emitted C function. The proc
# capture scanner walked `if`/`while` branches but not `case` arms
# (they hang off @nd_conditions), so a branch-only local was never
# classified — neither captured nor declared — and the lambda's C
# function referenced an undeclared `lv_<name>`. Plain method and
# `.each {}` bodies were unaffected.

# 1. case-branch local, read after the branch.
f = lambda do |t|
  case t
  when :a
    x = 10
    x + 1
  end
end
puts f.call(:a)                   #=> 11

# 2. proc form, same shape.
g = proc do |t|
  case t
  when :b
    y = 20
    y * 2
  end
end
puts g.call(:b)                   #=> 40

# 3. Closure mutation inside a case branch updates the captured
#    outer variable (not a shadowing proc-local).
acc = 0
h = lambda do |t|
  case t
  when :a
    acc = acc + 10
  when :b
    acc = acc + 100
  end
  acc
end
h.call(:a)
h.call(:b)
puts acc                          #=> 110

# 4. if-branch local in a lambda (the already-working sibling path,
#    kept as a guard).
k = lambda do |n|
  if n > 0
    m = n * 3
    m + 1
  else
    0
  end
end
puts k.call(4)                    #=> 13

puts "done"
