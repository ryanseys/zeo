# webrick is a pure-Ruby bundled gem (small HTTP server, no C extension)
# that isn't vendored under gems/ -- `require "webrick"` raises LoadError.
require "webrick"
p defined?(WEBrick)
