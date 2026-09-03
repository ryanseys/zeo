h = {}
puts h.compare_by_identity?          # false
puts h.compare_by_identity.equal?(h) # true (returns self)
puts h.compare_by_identity?          # true
a = "x" + ""
b = "x" + ""
h[a] = 1
h[b] = 2
puts h.size                          # 2 (distinct identities)
puts h[a]                            # 1
p h["x" + ""]                        # nil (fresh object)
# immediates still key by value
hi = {}.compare_by_identity
hi[1] = "one"; hi[1] = "ONE"
hi[:s] = 9; hi[:s] = 10
puts hi.size                         # 2 (1 and :s)
puts hi[1]                           # ONE
# frozen raises
begin
  {}.freeze.compare_by_identity
rescue => e
  puts e.class                       # FrozenError
end
__END__
false
true
true
2
1
nil
2
ONE
FrozenError
