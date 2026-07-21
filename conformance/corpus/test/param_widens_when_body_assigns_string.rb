# A method body that reassigns one of its parameters to a value
# of an incompatible type (here: int call sites + string body
# write) needs the param's slot widened to `poly` so the
# generated C can hold both shapes via sp_RbVal. Without the
# widening the parameter slot stays at the call-site-inferred
# `mrb_int`, and the body's `lv_x = "forever"` assignment is
# `mrb_int = const char *` — rejected by gcc under
# `-Wint-conversion`.

class Logger
  def report(label, hclk)
    hclk = "forever" if hclk == 4294967295
    puts label
    puts hclk
  end
end

l = Logger.new
l.report("running", 1234)
l.report("running", 4294967295)
l.report("done", 17)
