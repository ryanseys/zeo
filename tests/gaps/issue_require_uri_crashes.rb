# A bare `require "uri"` crashes at load time (before any URI functionality
# is used): the vendored gem's top level calls `Module#const_defined?` with
# the two-argument form (see issue_const_defined_two_arg.rb), which zeo
# doesn't support.
require "uri"
puts "loaded ok"
