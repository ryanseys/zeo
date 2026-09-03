# `load "file", wrap` needs load-time anonymous-module scoping this compiler
# lacks, so the wrap form lowers to a runtime `Kernel#load` that raises
# LoadError (the literal no-wrap form is spliced and executed instead).

Dir.chdir(File.expand_path("load_with_a_wrap_argument_lowers_to_a_runtime_loaderror", __dir__))
require_relative "load_with_a_wrap_argument_lowers_to_a_runtime_loaderror/main"
__END__
1
