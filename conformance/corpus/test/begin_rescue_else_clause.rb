# Phase 1B: `begin..rescue..else..end` the `else` body runs only when
# the begin body completed without exception. Pre-fix the parser
# emitted the else_clause field but the codegen ignored it -- the
# else body was silently dropped from emit.
#
# Covered:
#   - else with no exception (else runs)
#   - else with exception (else does NOT run)
#   - else combined with ensure
#   - method-level def with begin..rescue..else

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
