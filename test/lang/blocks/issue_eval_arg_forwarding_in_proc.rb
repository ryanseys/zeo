# `def m(...)` inside a proc built by `eval` -- forwardable's `def_delegators`
# generates exactly this and `module_eval`s the result, so every gem that
# delegates (csv, others) depends on it. The eval-VM's parser rejects the
# `(...)` forwarding here and reports `unexpected 'do', ignoring it`.
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
