# Test instance_eval { block } block-form lifting.
# Each section's output is compared against CRuby by `make test`.

class Config
  attr_accessor :port, :host, :debug

  def initialize
    @port = 0
    @host = ""
    @debug = false
  end
end

# ---- 1. Top-level basic form ----
cfg = Config.new
cfg.instance_eval do
  self.port = 8080
  self.host = "localhost"
  self.debug = true
end
puts cfg.port    # 8080
puts cfg.host    # localhost
puts cfg.debug   # true

# ---- 2. Two instance_eval calls in sequence (same object) ----
cfg.instance_eval do
  self.port = 9090
end
puts cfg.port    # 9090

# ---- 3. Methods called inside the block dispatch through self ----
class Routes
  attr_accessor :entries

  def initialize
    @entries = "init".split(",")  # StrArray hint
    @entries.pop                  # start empty
  end

  def get(path)
    @entries.push("GET " + path)
  end

  def post(path)
    @entries.push("POST " + path)
  end
end

app = Routes.new
app.instance_eval do
  get("/")
  get("/about")
  post("/login")
end
i = 0
while i < app.entries.length
  puts app.entries[i]
  i = i + 1
end

# ---- 4. instance_eval inside a top-level while loop ----
counter = Config.new
i = 0
while i < 3
  counter.instance_eval do
    self.port = self.port + 1
  end
  i = i + 1
end
puts counter.port  # 3

# ---- 5. instance_eval inside a top-level if branch ----
flag = Config.new
cond = 1
if cond > 0
  flag.instance_eval do
    self.debug = true
  end
end
puts flag.debug  # true

# ---- 6. Different objects of different classes interleaved ----
a = Config.new
b = Routes.new
a.instance_eval { self.port = 1 }
b.instance_eval { get("/x") }
a.instance_eval { self.port = 2 }
b.instance_eval { post("/y") }
puts a.port              # 2
puts b.entries.length    # 2
puts b.entries[0]        # GET /x
puts b.entries[1]        # POST /y

# ---- 7. Reassignment to another instance of the same class ----
fresh = Config.new
fresh.instance_eval { self.port = 11 }
puts fresh.port  # 11
fresh = Config.new
fresh.instance_eval { self.port = 22 }
puts fresh.port  # 22

# ---- 8. Receiver from instance variable inside a class method ----
# v2 wider-receiver (ivars): @ivar receiver. cls_ivar_type returns the
# ivar's stored type; @current_class_idx is set by ieval_walk_class_methods
# when entering each class's instance method bodies, so the lift can
# resolve `@routes` to obj_Routes without going through a local copy.
class Boot
  attr_accessor :routes

  def initialize
    @routes = Routes.new
  end

  def install
    @routes.instance_eval do
      get("/ivar")
      post("/ivar")
    end
  end
end

boot = Boot.new
boot.install
puts boot.routes.entries.length  # 2
puts boot.routes.entries[0]      # GET /ivar
puts boot.routes.entries[1]      # POST /ivar

# ---- 9. Ivar receiver in tail position of a class method ----
# Same lift but the instance_eval call is the method body's last
# expression. The block's last call (`get("/tail")`) returns the
# entries array, so the lift's value would flow out; explicitly
# return @routes to keep `setup` returning the receiver.
class Configure
  attr_accessor :routes

  def initialize
    @routes = Routes.new
  end

  def setup
    @routes.instance_eval { get("/tail") }
    @routes
  end
end

cfgr = Configure.new
ret = cfgr.setup
puts ret.entries.length  # 1
puts ret.entries[0]      # GET /tail

# ---- 10. Receiver from a method param ----
# v2 wider-receiver (params): the param's class comes from the caller,
# not the body. infer_param_types_from_callsites widens Wire#wire_param's
# `r` ptype from "int" to obj_Routes when it sees `w.wire_param(shared)`
# at top level (with `w` declared in scope so the receiver-method
# branch fires). ieval_walk_class_methods then declares `r: obj_Routes`
# in scope, and find_var_type resolves the LocalVariableReadNode
# receiver inside the method body.
class Wire
  def wire_param(r)
    r.instance_eval do
      get("/param")
    end
  end
end

w = Wire.new
shared = Routes.new
w.wire_param(shared)
puts shared.entries.length  # 1
puts shared.entries[0]      # GET /param

# ---- 11. Param receiver in tail position ----
# Same as §10 but the lift is the method body's last expression.
# Block's last call (`get("/tail-param")`) returns the entries
# array; explicitly return `r` to keep wire_param returning the
# receiver.
class WireTail
  def wire_param(r)
    r.instance_eval { get("/tail-param") }
    r
  end
end

w2 = WireTail.new
shared2 = Routes.new
ret2 = w2.wire_param(shared2)
puts ret2.entries.length  # 1
puts ret2.entries[0]      # GET /tail-param

# ---- 12. Method-local copy of a class instance ----
# scan_locals_first_type sees `routes = Routes.new` and declares
# `routes : obj_Routes` in scope. The method-param case (§10) and
# this method-local case both flow through the same find_var_type
# fallback added to ieval_rewrite_call's LocalVariableReadNode branch.
class WireLocal
  def setup
    routes = Routes.new
    routes.instance_eval do
      get("/local")
      post("/local")
    end
    routes
  end
end

ret3 = WireLocal.new.setup
puts ret3.entries.length  # 2
puts ret3.entries[0]      # GET /local
puts ret3.entries[1]      # POST /local
