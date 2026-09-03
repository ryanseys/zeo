t = Time.utc(2026, 7, 16, 13, 45, 30)
p(t == 5); p(t != 5); p(t <=> 5); p(t == "x")
a = Time.utc(2000,1,1); b = Time.utc(2030,1,1)
p(a < b); p(a <=> b); p(a == a); p(a <=> "z")
begin; a < 5; rescue ArgumentError; puts "argerr"; end
__END__
false
true
nil
false
true
-1
true
nil
argerr
