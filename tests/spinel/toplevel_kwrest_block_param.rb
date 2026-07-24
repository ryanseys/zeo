class App
  def initialize(title:, body:)
    @title = title
    @body = body
  end

  def render
    puts @title
    @body.call
  end
end

# Top-level def combining `**kwrest` with `&block`, passing the block local
# as a kwarg to a `.new` call site.
def app(title = nil, **_chrome, &block)
  App.new(title: title || "default", body: block)
end

app("hello") { puts "ran" }.render
