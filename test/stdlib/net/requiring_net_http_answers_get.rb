# The vendored gem loads. net/http has no C extension of its own; it builds on
# the socket one.
require "net/http"
p Net::HTTP.respond_to?(:get)
__END__
true
