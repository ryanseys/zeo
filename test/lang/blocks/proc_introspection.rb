# Issue #861: Proc#arity, #lambda?, and #parameters dispatch.

p = Proc.new { |a, b| }
puts p.arity
puts p.lambda?
puts p.parameters.inspect

l = lambda { |a, b| }
puts l.arity
puts l.lambda?
puts l.parameters.inspect
__END__
2
false
[[:opt, :a], [:opt, :b]]
2
true
[[:req, :a], [:req, :b]]
