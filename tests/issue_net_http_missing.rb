# `require "net/http"` -- the vendored gem loads and `Net::HTTP.get` answers.
# net/http has no C extension of its own; it builds on the socket ext.
require "net/http"
p Net::HTTP.respond_to?(:get)
