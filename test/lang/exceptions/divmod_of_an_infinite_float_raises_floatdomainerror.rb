# (1.0/0.0).divmod raises FloatDomainError, and the message names the value.
def t
  yield
rescue FloatDomainError => e
  e.message
end
puts t { (1.0/0.0).divmod(2) }
puts t { (-1.0/0.0).divmod(2) }
puts t { (0.0/0.0).divmod(2) }
p (7.5).divmod(2)
p (7.5).divmod(2.5)
p (-7.5).divmod(2)
__END__
Infinity
-Infinity
NaN
[3, 1.5]
[3, 0.0]
[-4, 0.5]
