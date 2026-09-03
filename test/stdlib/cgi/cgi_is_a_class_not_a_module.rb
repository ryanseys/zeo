# CRuby's `CGI` is a CLASS whose superclass is `Object` -- `CGI.new(...)` is the
# whole CGI API -- and cgi/escape.rb opens it as one (`class CGI`). zeo had it
# registered as a module, so every gem that reopened it stopped at `CGI is not a
# class`, and thin's `class CGIWrapper < ::CGI` had nothing to subclass.

require "cgi/escape"

p [CGI.class, CGI.superclass]
p [CGI.is_a?(Class), CGI.is_a?(Module), CGI.instance_of?(Class)]

# The escape methods answer on the class, as they do in ruby.
p CGI.escape("a b&c=d")
p CGI.unescape("a+b%26c")
p CGI.escapeHTML(%(Usage: foo "bar" <baz>))
p CGI.unescapeHTML("&lt;x&gt;&amp;&quot;y&quot;")
p CGI.escapeURIComponent("a b&c")
p CGI.unescapeURIComponent("a%20b%26c")

# The snake_case aliases are the same methods.
p [CGI.escape_html("<x>"), CGI.unescape_html("&lt;x&gt;")]
p [CGI.escape_uri_component("a b"), CGI.unescape_uri_component("a%20b")]

# Being a class means it can be SUBCLASSED, which is what thin does to wrap a
# rails request. A subclass is an ordinary ivar-carrying object: CGI itself
# holds no payload.
class CGIWrapper < ::CGI
  def initialize(env)
    @env = env
  end

  attr_reader :env

  def escaped_path
    CGI.escape(@env[:path])
  end
end

w = CGIWrapper.new(path: "/a b/c")
p [CGIWrapper.superclass, w.env, w.escaped_path]
p [w.is_a?(CGI), w.is_a?(Object), CGIWrapper.ancestors.first(2)]

# A subclass may add to what it inherits, and `super` reaches Object's own.
class Loud < CGIWrapper
  def escaped_path
    super.upcase
  end
end

p Loud.new(path: "/x y").escaped_path
p [Loud.superclass, Loud.ancestors.first(3)]

# `CGI` names a real class, so it works as a constant, in a rescue list
# position, and as a `===` subject the way any class does.
p [CGI.name, CGI.to_s]
p [CGI === CGIWrapper.new(path: "/"), CGI === "not a cgi"]
p CGIWrapper.new(path: "/").instance_variables
__END__
[Class, Object]
[true, true, true]
"a+b%26c%3Dd"
"a b&c"
"Usage: foo &quot;bar&quot; &lt;baz&gt;"
"<x>&\"y\""
"a%20b%26c"
"a b&c"
["&lt;x&gt;", "<x>"]
["a%20b", "a b"]
[CGI, {path: "/a b/c"}, "%2Fa+b%2Fc"]
[true, true, [CGIWrapper, CGI]]
"%2FX+Y"
[CGIWrapper, [Loud, CGIWrapper, CGI]]
["CGI", "CGI"]
[true, false]
[:@env]
