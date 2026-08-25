# `(10**30).times` counts from 0 like any receiver: ruby yields lazily, so an
# early `break` works even when the full count is physically unrunnable. A
# negative receiver yields nothing and answers itself.
got = []
r = (10**30).times do |i|
  got << i
  break :early if i >= 2
end
p got
p r
p((-(10**30)).times { raise "unreachable" })
p (10**30).times.first(2)
