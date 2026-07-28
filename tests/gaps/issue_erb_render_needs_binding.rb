# Rendering an ERB template evaluates the compiled source in a Binding:
# `ERB#result`/`#result_with_hash` default to `new_toplevel`, which derives
# a binding from `TOPLEVEL_BINDING`. zeo has neither the constant nor
# `Kernel#binding`, so the vendored gem loads and compiles but cannot render.
require "erb"

p ERB.new("<%= 1 + 1 %> and <%= name %>").result_with_hash(name: "zeo")
p ERB.new("<%= 2 * 3 %>").result(binding)
