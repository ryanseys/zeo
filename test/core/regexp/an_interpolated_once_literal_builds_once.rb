# `/o` builds an interpolated regexp once per site: later evaluations answer
# the same frozen object, and the interpolations never run again. Two sites
# are two objects. A build that raises keeps nothing, so the next evaluation
# builds again. A bare condition (`/#{d}/o` against `$_`) builds every time.
calls = 0
next_part = -> { calls += 1; calls.to_s }
res = 3.times.map { /x#{next_part.()}/o }
p res, calls, res.map(&:object_id).uniq.size, res[0].frozen?

def site(v) = /#{v}/o
p site(1), site(2)
p [/#{1}/o.equal?(/#{1}/o)]

pats = ["(", "ok"]
2.times do
  p(/#{pats.shift}/o)
rescue RegexpError => e
  p e.class
end

$_ = "line 9"
hits = %w[9 x].map { |d| (/#{d}/o ? 1 : 0) }
p hits
__END__
[/x1/, /x1/, /x1/]
1
1
true
/1/
/1/
[false]
RegexpError
/ok/
[1, 0]
