p Integer.sqrt(0)
p Integer.sqrt(8)
p Integer.sqrt(9)
p Integer.sqrt(10**20)
p Integer.sqrt(2**100)
begin
  Integer.sqrt(-4)
rescue Math::DomainError => e
  puts e.message
end
__END__
0
2
3
10000000000
1125899906842624
Numerical argument is out of domain - "isqrt"
