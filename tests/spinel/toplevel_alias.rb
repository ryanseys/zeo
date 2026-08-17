# A top-level `alias b a`: the alias collector only walked class and module
# bodies, so the new name was never registered and calling it raised NameError.
def t1; 1; end
alias t2 t1
p t2
p t1

def add(a, b); a + b; end
alias plus add
p plus(2, 3)
p add(2, 3)

def greet(n = "x"); "hi #{n}"; end
alias hello greet
p hello
p hello("y")

