# `begin..rescue..else..end`: the else body runs only when the begin body
# completed without an exception. With and without a raise, combined with
# ensure, and inside a def.

# 1. No exception -- else runs.
begin
  x = 1
rescue
  puts "rescued"
else
  puts "else ran"
end

# 2. Exception -- else skipped, rescue runs.
begin
  raise "boom"
rescue
  puts "rescued boom"
else
  puts "should not appear"
end

# 3. else + ensure: order is body -> (else | rescue) -> ensure.
begin
  y = 2
rescue
  puts "rescue"
else
  puts "else"
ensure
  puts "ensure"
end

# 4. Method-level: else's last expr is the method return value
# (overriding begin body's). When else's last is `puts ...` which
# returns nil, the method returns nil (spinel's int slot lowers it
# to 0).
def m_else_succeeds
  10
rescue
  -1
else
  puts "else fired"
end
puts m_else_succeeds       # "else fired" then 0 (spinel int slot for nil)

# 5. Method-level: exception path skips else, rescue value returned.
def m_else_skipped
  raise "x"
  10
rescue
  -1
else
  puts "should not fire"
end
puts m_else_skipped        # -1
__END__
else ran
rescued boom
else
ensure
else fired

-1
