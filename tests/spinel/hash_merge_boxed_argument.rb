# `{ sym: v }.merge(opts)` where opts only reads poly: the argument holds
# whichever hash variant the value really is, so casting it to the receiver's
# layout read one struct through another -- a Symbol key was then dereferenced
# as a char * (#3975).
module VH
  def self.render(attrs)
    out = +""
    attrs.each { |k, v| out << " #{k}=#{v}" }
    out
  end

  def self.sheet(href, opts) = render({ rel: "stylesheet", href: href }.merge(opts))
  def self.button(opts)      = render({ type: "submit" }.merge(opts))
  def self.form(href, opts)  = render({ action: href, method: "post" }.merge(opts))
  def self.raw(attrs)        = render(attrs)
end

p VH.sheet("app.css", {})
p VH.button({ "class" => "btn" })
p VH.form("/x", { id: "f" })
p VH.raw({ "data-id" => "7" })

# a Hash#default(key) whose key type cannot be one of this hash's keys answers
# the plain default rather than handing the wrong pointer to the default proc
p Hash.new(5).default(4)
p({}.default(4))
p({}.default)
d = Hash.new { |h, k| "made-#{k}" }
p d.default("x")
p d.default
