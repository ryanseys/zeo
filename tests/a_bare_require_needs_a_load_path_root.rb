# `require "thing"` names the `thing.rb` sitting under a `$LOAD_PATH` root --
# not every loaded file whose path happens to end in `/thing.rb`. zeo compared
# `$LOADED_FEATURES` by path SUFFIX alone, so a vendored copy already loaded
# under its own long path claimed the bare name, the require answered `false`,
# and the real file never ran.
#
# bundler vendors fileutils at `bundler/vendor/fileutils/lib/fileutils.rb` and
# loads it by `require_relative`. rubygems' `Gem.open_file_with_lock` then
# writes `require "fileutils"; FileUtils.rm_f`, and that require answered
# `false` against the vendored path -- so `FileUtils` was undefined and
# `bundle install` raised on its last step.

require_relative "a_bare_require_needs_a_load_path_root/vendor/thing/lib/thing"
p Vendored::THING
$LOAD_PATH.unshift File.expand_path("a_bare_require_needs_a_load_path_root/lib", __dir__)
p require("thing")
p Thing::WHICH
p require("thing")
