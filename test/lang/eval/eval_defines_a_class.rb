# A literal `eval` splices at compile time, and a `class` at its top level
# registers exactly as one written in place -- the registration walk descends
# the spliced body. (A top-level `def` in an eval is still a rejection.)
eval("class EvalKlass\n  def hi\n    41 + 1\n  end\nend")
p EvalKlass.new.hi
p EvalKlass.instance_methods(false).sort
__END__
42
[:hi]
