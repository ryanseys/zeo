# Every operand of a call stays alive until the call runs. An operand that is a
# fresh allocation is reachable from nothing until the callee stores it, so a
# sibling operand evaluated afterwards can collect it -- the argument-position
# shape #4049 fixed for a constructor's members, which every other call had too.
# Each operand binds to a rooted temp in front of the call now. A natural
# collection does not reliably land in that window, so the lifetime half is
# verified under SPINEL_GC_STRESS=1, where the first count was 40 of 40 before
# the fix and 0 after. What this run asserts unconditionally is the shape
# and the answer.
def seg(tag, i)
  s = String.new(tag)
  s + i.to_s
end

def churn(tag, i)
  parts = []
  40.times { |j| parts.push("#{tag}-#{i}-#{j}......................") }
  GC.start
  parts.length
end

# allocates, collects, and answers a String: as a later operand it is what
# reclaims an earlier one
def churn_seg(tag, i)
  churn(tag, i)
  s = String.new(tag)
  s + i.to_s
end

bad = 0
40.times do |i|
  joined = File.join(seg("a", i), churn_seg("m", i), seg("b", i))
  bad += 1 unless joined == "a#{i}/m#{i}/b#{i}"
end
p bad

# the allocating sibling sits between two fresh operands, so one of them is
# evaluated before it whichever order a C compiler would have picked
bad2 = 0
40.times do |i|
  left = seg("L", i)
  n = churn("m", i)
  right = seg("R", i)
  bad2 += 1 unless "#{left}/#{n}/#{right}" == "L#{i}/40/R#{i}"
end
p bad2

bad3 = 0
40.times do |i|
  joined = [seg("x", i), churn("y", i).to_s, seg("z", i)].join(",")
  bad3 += 1 unless joined == "x#{i},40,z#{i}"
end
p bad3
