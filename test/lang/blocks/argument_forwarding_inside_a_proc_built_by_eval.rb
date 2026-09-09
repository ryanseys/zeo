# `def m(...)` inside an eval'd proc, which is what forwardable's
# def_delegators generates and module_evals.
class C; end
src = "proc do\n  def a3(...)\n    _ = \"z\"\n    _.upcase(...)\n  end\nend"
C.module_eval(&eval(src))
p C.new.a3

# The same method spelled without forwarding compiles fine, which is what
# narrows the blame to `(...)`.
class D; end
D.module_eval(&eval("proc do\n  def ok(*args)\n    \"z\".upcase(*args)\n  end\nend"))
p D.new.ok
__END__
"Z"
"Z"
