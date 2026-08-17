# A proc's arity: only a rest parameter makes it negative. Optional keywords
# and a keyword rest were treated as negative for a proc too, so
# `proc { |**kw| }.arity` answered -1 where CRuby answers 0.
p [proc{|**k|}.arity, proc{|a,**k|}.arity, lambda{|a,**k|}.arity]
p [proc{|b:1|}.arity, proc{|a,b:1|}.arity, proc{|a:|}.arity, lambda{|a:|}.arity]
p [proc{|a,b=1|}.arity, lambda{|a,b=1|}.arity, proc{|a,*r|}.arity, proc{|a:,b:1|}.arity]
p [lambda{|b:1|}.arity, proc{|*r|}.arity, lambda{|*r|}.arity, proc{|**k,&b|}.arity]
p [lambda{|a,b:1|}.arity, proc{|a,b,c=1|}.arity, lambda{|a,b,c=1|}.arity]
p [lambda{|a:,b:1|}.arity, lambda{|a:,**k|}.arity, proc{|a:,**k|}.arity]
p [proc{|a|}.arity, proc{|a,b|}.arity, lambda{|a,b|}.arity, proc{}.arity, lambda{}.arity]
