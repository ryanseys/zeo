# `rescue => e` inside a lambda / proc body binds `e` as a body-local
# exception. Before the fix the binding leaked to the enclosing scope so
# the closure referenced an undeclared `lv_e`, and infer_type recomputed
# `e.message` as int (a `#{e.message}` interp formatted the string
# pointer with %lld). The string is observed inside the body so the test
# is independent of the proc-call return-value ABI.
lambda { begin; raise "a"; rescue => e; puts "got #{e.message}"; end }.call
proc { begin; raise "boom"; rescue => e; puts "p #{e.message} (#{e.class})"; end }.call

# Same construct, stored then called.
f = lambda { begin; raise "late"; rescue => e; puts "f: #{e.message}"; end }
f.call

# A non-raising path through the body still compiles and runs.
g = proc { begin; puts "no raise"; rescue => e; puts "unreached #{e.message}"; end }
g.call
