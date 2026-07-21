# EmbeddedVariableNode -- `"#$gv"`, `"#@iv"`, `"#@@cv"`.
#
# Shorthand interpolation form that skips the braces around a
# bare variable reference. Lowered at parser side to an
# EmbeddedStatementsNode wrapping the matching variable read,
# so the codegen reuses the existing interpolation path.

$global = "global!"
puts "got: #$global"          # got: global!

class C
  def initialize
    @ivar = "ivar!"
  end
  def show = puts("got: #@ivar")
end
C.new.show                    # got: ivar!
