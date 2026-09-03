# A `*args` positional splat or a `**opts` double-splat at a `.new` call
# site expands at runtime, dispatching through the class's constructor --
# the same as any ordinary splat call.
class Point
  def initialize(x, y)
    @x = x
    @y = y
  end

  def to_s
    "(#{@x}, #{@y})"
  end
end

coords = [3, 4]
puts Point.new(*coords)              # (3, 4)

# `Data.define` classes construct by keyword, so a `**hash` double-splat
# forwards a runtime hash straight into the constructor.
Config = Data.define(:host, :port)
opts = { host: "localhost", port: 8080 }
p Config.new(**opts)                 # #<data Config host="localhost", port=8080>
p Config.new(**{ host: "example.com", port: 443 })
__END__
(3, 4)
#<data Config host="localhost", port=8080>
#<data Config host="example.com", port=443>
