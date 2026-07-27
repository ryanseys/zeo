# a branch that raises contributes no type to the case/if it sits in
def enc(o)
  case o
  when Integer then 7
  when String  then 12
  else raise ArgumentError, "no"
  end
end
v = enc("ab")
p v.class
p v.succ
p v.succ.class

def s(o)
  case o
  when Integer then "i"
  when String  then "sample text"
  else raise ArgumentError, "no"
  end
end
p s("ab").count("s")
p s("ab").sum
p s("ab").match?(/\w/)

def enc2(o)
  if o.is_a?(String) then "sample text" else raise ArgumentError, "no" end
end
p enc2("ab").match?(/\w/)

def h(o)
  case o
  when Integer then {"a" => 1}
  when String  then {"b" => 2}
  else raise ArgumentError, "no"
  end
end
p h("ab").sum { |_k, v| v }

# the raising branch is still taken when it should be
r = (enc(1.5) rescue $!.class)
p r
