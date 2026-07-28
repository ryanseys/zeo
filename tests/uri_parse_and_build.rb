# `require "uri"` -- the vendored gem loads and parses. Covers `module Kernel;
# def URI(u); module_function :URI`, whose two halves share one generated
# container; `defined?(::URI::Parser)` guarding the `const_set` that creates it;
# and `Parser.new` resolving that runtime constant through the lexical cref.
require "uri"

u = URI("https://user@example.com:8443/a/b?q=1#frag")
p [u.scheme, u.host, u.port, u.userinfo, u.path, u.query, u.fragment]
p u.class

p URI.parse("http://example.com/x").request_uri
p URI.join("http://example.com/a/", "b").to_s
p URI.encode_www_form(a: 1, b: "x y")
p URI.decode_www_form("a=1&b=x+y")
p URI::DEFAULT_PARSER.class
p defined?(::URI::Parser)
