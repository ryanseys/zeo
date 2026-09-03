# `Method#original_name` / `UnboundMethod#original_name` answer the name a
# method was DEFINED under, which differs from `#name` only when it was reached
# through an alias. `#inspect` shows the same fact in parens: `A#y(x)`.
#
# There are four ways a method can arrive under a second name, and all four
# have to remember where they came from.

def shape(m) = m.inspect.sub(/ [^ ]+:\d+>\z/, ">")

# --- 1. an alias in the SAME body as its source ------------------------------
class A
  def x(q) = q
  alias_method :y, :x
  alias z x
end
a = A.new
p a.method(:x).name, a.method(:x).original_name
p a.method(:y).name, a.method(:y).original_name
p a.method(:z).original_name
p shape(a.method(:y))

# The alias is a real method: it calls, and it keeps the source's signature.
p a.method(:y).call(1), a.method(:y).arity, a.method(:y).parameters

# --- 2. an alias of an INHERITED method (a different body) -------------------
class B < A
  alias_method :w, :x
end
b = B.new
p b.method(:w).name, b.method(:w).original_name
p b.method(:w).owner
p shape(b.method(:w))
p b.method(:w).call(2)

# A subclass INHERITS the alias-ness of an alias defined above it.
p B.new.method(:y).original_name

# --- 3. an alias whose source is a BUILTIN -----------------------------------
class C
  alias_method :inspect_it, :inspect
  alias_method :len, :object_id
end
p C.new.method(:inspect_it).name, C.new.method(:inspect_it).original_name
p C.new.method(:len).original_name

# --- 4. an alias made at RUNTIME ---------------------------------------------
class D
  def hello(who) = "hi #{who}"
end
D.class_eval { alias_method :greet, :hello }
d = D.new
p d.method(:greet).name, d.method(:greet).original_name
p d.method(:greet).call("ada")
p d.method(:greet).parameters

# A chain resolves all the way back to the original, never to the middle.
D.class_eval { alias_method :salute, :greet }
p D.new.method(:salute).original_name

# --- an ordinary method reports its own name ---------------------------------
p a.method(:x).original_name == a.method(:x).name
p 1.method(:+).original_name
p [].method(:each).original_name

# --- UnboundMethod answers identically ---------------------------------------
p A.instance_method(:y).name, A.instance_method(:y).original_name
p shape(A.instance_method(:y))
p B.instance_method(:w).original_name
p A.instance_method(:x).original_name
__END__
:x
:x
:y
:x
:x
"#<Method: A#y(x)(q)>"
1
1
[[:req, :q]]
:w
:x
B
"#<Method: B(A)#w(x)(q)>"
2
:x
:inspect_it
:inspect
:object_id
:greet
:hello
"hi ada"
[[:req, :who]]
:hello
true
:+
:each
:y
:x
"#<UnboundMethod: A#y(x)(q)>"
:x
:x
