# A BOX requires out of the pack too, and gets its own private copy.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1
#@ zeo: --embed-sources features/a_box_requires_out_of_the_embedded_pack/lib

$LOAD_PATH.unshift(File.expand_path("a_box_requires_out_of_the_embedded_pack/lib", __dir__))
require_relative "a_box_requires_out_of_the_embedded_pack/prog"
__END__
#@ stderr
features/a_box_requires_out_of_the_embedded_pack.rb:3:in '<main>': uninitialized constant Greeter (NameError)
#@ exit 1
