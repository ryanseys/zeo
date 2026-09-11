# One file demanded twice under two different names: an `autoload` naming it
# on the load path, and a guarded `require_relative` naming it by path. rack
# writes both, and the second never runs. The file loads once, when the
# autoload is touched, and the constant it defines is the one the program
# reads.

$LOAD_PATH.unshift(File.expand_path("an_autoload_and_a_guarded_require_of_one_file_share_its_unit/lib", __dir__))
require_relative "an_autoload_and_a_guarded_require_of_one_file_share_its_unit/prog"
__END__
pkg ran
before read
target ran
:split
