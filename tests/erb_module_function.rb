# `require "erb"` -- the vendored gem loads and templates render. `erb/util.rb`
# reaches `html_escape` through `include ERB::Escape` and then
# `module_function :html_escape`, so it exercises `module_function` naming a
# method this body did not define.
require "erb"

p ERB::Util.html_escape("a > b & c")
p ERB::Util.h("<x>")
p ERB::Util.url_encode("a b")
# Rendering a template needs `TOPLEVEL_BINDING` -- see
# `tests/gaps/issue_erb_render_needs_binding.rb`.

# `module_function` keeps BOTH halves: a public module method and a private
# instance method for the include-mixin.
module Greet
  def hello(who) = "hi #{who}"
end

module Util
  include Greet
  module_function :hello
end

class User
  include Util
end

p Util.hello("a")
p User.new.respond_to?(:hello)
p User.new.send(:hello, "b")
