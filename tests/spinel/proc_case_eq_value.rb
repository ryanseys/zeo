# Proc#=== answers the proc's return value; a case/when only cares about its
# truthiness, so both readings agree there but a direct call does not (#3818).
double = ->(x) { x * 2 }
p(double === 5)
pred = ->(x) { x > 3 }
p(pred === 5)
p(pred === 1)
v = 5
case v
when ->(x) { x > 3 } then puts "big"
else puts "small"
end
box = [->(x) { x.to_s }, 1]
f = box[0]
p(f === 7)
