# net/http vendored. No connection is made here -- what this pins is that the
# gem loads and its request/header/URI plumbing is usable offline. HTTPS needs
# a real OpenSSL (see docs/COMPATIBILITY.md).
require "net/http"

p Net::HTTP.superclass
p Net::HTTP.default_port, Net::HTTP.https_default_port

http = Net::HTTP.new("example.test", 8080)
p http.address, http.port, http.started?
p http.use_ssl?

req = Net::HTTP::Get.new("/index?a=1")
p req.method, req.path
req["Accept"] = "text/plain"
p req["accept"]
p req.each_header.to_a.sort

post = Net::HTTP::Post.new("/submit")
post.set_form_data("k" => "v", "n" => "1")
p post.body
p post.content_type

p Net::HTTP::Head.new("/").request_body_permitted?
p Net::HTTP::Post.new("/").request_body_permitted?

uri = URI("http://example.test:8080/p?q=1")
p Net::HTTP::Get.new(uri).path
p Net::HTTPResponse.ancestors.include?(Net::HTTPHeader)
p Net::HTTPNotFound.superclass
