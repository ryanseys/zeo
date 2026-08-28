# The value of `begin ... rescue ... else ... end`: the else clause's on
# success (including an empty else), the rescue arm's on an exception, in
# expression and method-tail position and inside a block. CRuby generated
# the expectations.
a = begin; :body; rescue; :r; else; false; end
p a
b = begin; :body; rescue; :r; else; 3; end
p b
c = begin; 1; rescue; 2; else; nil; end
p c
d = begin; 1; rescue; 2; else; 42; end
p d
e = begin; "s"; rescue; "r"; else; "e"; end
p e
f = begin; raise "x"; rescue; :r; else; :e; end
p f
def g; begin; 1; rescue; 2; else; false; end; end
p g
h = begin; 1; rescue; 2; else; 7; ensure; 99; end
p h
i = begin; :body; rescue; :r; end
p i
def j(n)
  begin
    Integer(n)
  rescue ArgumentError
    :bad
  else
    [n, true]
  end
end
p j("1")
p j("x")
k = [1, 2].map { |v| begin; v; rescue; 0; else; v.to_s; end }
p k
m = begin; 1; rescue; 2; else; end
p m
def n; begin; :x; rescue; :r; else; end; end
p n
