# A negative Float raised to a fractional power has no real answer, so the
# result is a Complex rather than a NaN or a raise. An integral exponent on
# the same negative base stays real.
begin
  p((-2.0) ** 0.5)
rescue => e
  puts "#{e.class}: raised"
end
p(2.0 ** 0.5)
p((-2.0) ** 2.0)
p((-8.0) ** (1.0 / 3)) rescue puts "raised2"
__END__
(0.0+1.4142135623730951i)
1.4142135623730951
4.0
(1.0+1.7320508075688772i)
#@ linux stdout
(0.0+1.4142135623730951i)
1.4142135623730951
4.0
(1.0000000000000002+1.7320508075688772i)
