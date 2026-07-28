# net/http is a pure-Ruby default gem (built on top of socket, no C
# extension of its own) that isn't vendored under gems/ -- `require
# "net/http"` raises LoadError.
require "net/http"
p Net::HTTP.respond_to?(:get)
