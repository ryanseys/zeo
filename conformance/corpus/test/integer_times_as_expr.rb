# Issue #877: Integer#times / #upto / #downto in expression
# position return self (per MRI). Pre-fix, the expression form
# emitted 0; the statement form already iterated correctly.
x = 5.times { }
puts x
y = 1.upto(5) { }
puts y
z = 5.downto(1) { }
puts z
