# One file demanded twice under two different names: an `autoload` naming it
# on the load path, and a guarded `require_relative` naming it by path. rack
# writes both, and the second never runs.
#
# The compile cannot resolve `pkg/target` (the load path is set at run time),
# so the autoload is not joined to the unit the `require_relative` compiled.
# At run time the autoload finds no unit under its name and compiles a second
# copy of the file; the compiled `Pkg::Target` read stays bound to the first
# copy's class, which never ran, so the read raises NameError.

$LOAD_PATH.unshift(File.expand_path("an_autoload_and_a_guarded_require_of_one_file_share_its_unit/lib", __dir__))
require_relative "an_autoload_and_a_guarded_require_of_one_file_share_its_unit/prog"
__END__
pkg ran
before read
target ran
:split
