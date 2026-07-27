# OpenSSL is a CRuby C extension. `require "openssl"` loads without a
# LoadError, but the module is a bare stub -- even OpenSSL::VERSION is an
# uninitialized constant, so none of the actual crypto/TLS functionality is
# usable.
require "openssl"
p OpenSSL::VERSION
