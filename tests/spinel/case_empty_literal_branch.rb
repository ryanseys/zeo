# An empty `[]`/`{}` literal as a `case` branch value, merged with a branch
# of the same container kind, must build a real empty container rather than
# a null pointer statically typed as one.
r1 = case "s"
     when String then []
     else ["x"]
     end
p r1                    # []
r1.each { |e| puts e }  # nothing printed, no crash

r2 = case "s"
     when String then {}
     else { "a" => 1 }
     end
p r2                    # {}
r2.each { |k, v| puts "#{k}=#{v}" }  # nothing printed, no crash

# else-branch is the empty one.
p(case "s"
  when Integer then [1]
  else []
  end)                   # []

# The `case <int>` jump-table fast path lowers branch values through the same
# emitter as the if-chain form above; keep it covered separately.
def by_int(x)
  case x
  when 1 then []
  when 2 then [9]
  else [3]
  end
end
p by_int(1)              # []
p by_int(2)              # [9]
p by_int(3)              # [3]
by_int(1).each { |e| puts e }  # nothing printed, no crash

# A non-empty literal in the same position is unaffected (regression guard).
p(case "s"
  when String then ["a"]
  else ["b"]
  end)                   # ["a"]
