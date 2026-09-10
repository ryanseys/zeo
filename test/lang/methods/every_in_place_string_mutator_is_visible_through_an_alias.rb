# <<, upcase!, replace, slice! and insert on a local alias, and the same
# through a container and an ivar.
puts "== local alias =="
a1=+"x"; a2=a1; a1<<"1"; p a2
b1=+"x"; b2=b1; b1.upcase!; p b2
c1=+"x"; c2=c1; c1.replace("R"); p c2
d1=+"ax"; d2=d1; d1.slice!(0); p d2
e1=+"x"; e2=e1; e1.insert(0,">"); p e2
f1=+"x"; f2=f1; f1[0]="Z"; p f2
g1=+"x"; g2=g1; g1.clear; p g2
h1=+"Ab"; h2=h1; h1.setbyte(0,97); p h2
puts "== container =="
s=+"x"; arr=[s]; s<<"1"; s.upcase!; arr[0].reverse!; p arr; p s
h={k:+"v"}; h[:k]<<"1"; p h
puts "== ivar =="
class C1; def initialize; @s=+"x"; t=@s; @s<<"1"; p t; end; end; C1.new
puts "== param =="
def mm(x)=x.gsub!(+"x","G"); q=+"x"; r=q; mm(q); p r
puts "== capture =="
cs=+"x"; ct=cs; pr=->{ cs<<"c" }; pr.call; p ct
puts "== return =="
$g=[]; def mk; s=+"x"; $g<<s; s; end; rr=mk; rr<<"r"; p $g[0]
puts "== iteration =="
ls=[+"a",+"b"]; ls.each{|x| x<<"!"}; p ls
puts "== frozen =="
fz="x".freeze; fw=fz; (fz<<"!" rescue p :frozen); p fw
__END__
== local alias ==
"x1"
"X"
"R"
"x"
">x"
"Z"
""
"ab"
== container ==
["1X"]
"1X"
{k: "v1"}
== ivar ==
"x1"
== param ==
"G"
== capture ==
"xc"
== return ==
"xr"
== iteration ==
["a!", "b!"]
== frozen ==
:frozen
"x"
