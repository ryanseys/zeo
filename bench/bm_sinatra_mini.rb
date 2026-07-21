# Sinatra-mini: tiny HTTP-style router + handler dispatch. Adds a
# bench shape distinct from the existing three:
#   bm_jekyll_lite — string-heavy pipeline, 0 poly
#   bm_poly_cells  — synthetic polyvariance exerciser
#   bm_micro_lisp  — saturated polyvariance (Lisp eval)
#
# This one exercises: routing table (hash by path), per-route
# handler with different params shapes, params extraction from a
# query-string-shaped hash, response body construction. Shape is
# closer to a real web framework's hot path than the others, and
# the request/response cycle composition is a frequent source of
# poly-param widening in real codebases.
#
# Routes are explicit `if path == "..."` matches (Sinatra's
# regex/glob routing would require runtime pattern matching that's
# outside Spinel's subset). Handlers take the params hash and
# return a response body string.

class Request
  attr_accessor :method_name
  attr_accessor :path
  attr_accessor :params
  def initialize(method_name, path, params)
    @method_name = method_name
    @path = path
    @params = params
  end
end

class Response
  attr_accessor :status
  attr_accessor :body
  def initialize(status, body)
    @status = status
    @body = body
  end
end

def handle_index(req)
  Response.new(200, "Hello, world")
end

def handle_greet(req)
  name = req.params["name"]
  if name == ""
    name = "anonymous"
  end
  Response.new(200, "Hello, " + name)
end

def handle_echo(req)
  msg = req.params["msg"]
  count_str = req.params["count"]
  count = count_str.to_i
  if count <= 0
    count = 1
  end
  out = ""
  i = 0
  while i < count
    if i > 0
      out = out + " "
    end
    out = out + msg
    i = i + 1
  end
  Response.new(200, out)
end

def handle_not_found(req)
  Response.new(404, "Not Found: " + req.path)
end

class Router
  def initialize
    @log = []
  end

  def dispatch(req)
    @log.push(req.method_name + " " + req.path)
    if req.path == "/"
      return handle_index(req)
    end
    if req.path == "/greet"
      return handle_greet(req)
    end
    if req.path == "/echo"
      return handle_echo(req)
    end
    handle_not_found(req)
  end

  def log
    @log
  end
end

def render(res)
  res.status.to_s + " " + res.body
end

router = Router.new

reqs = [
  Request.new("GET", "/", { "name" => "" }),
  Request.new("GET", "/greet", { "name" => "Matz" }),
  Request.new("GET", "/greet", { "name" => "" }),
  Request.new("GET", "/echo", { "msg" => "hi", "count" => "3" }),
  Request.new("GET", "/echo", { "msg" => "x", "count" => "0" }),
  Request.new("GET", "/missing", { "name" => "" })
]

i = 0
while i < reqs.length
  res = router.dispatch(reqs[i])
  puts render(res)
  i = i + 1
end

puts "---log---"
entries = router.log
j = 0
while j < entries.length
  puts entries[j]
  j = j + 1
end
