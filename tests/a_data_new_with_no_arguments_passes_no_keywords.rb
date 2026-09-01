# `Data.new` zips its positionals into a hash and hands it to `initialize`
# AS KEYWORDS (`rb_class_new_instance_kw` + `RB_PASS_KEYWORDS`), so a user
# `initialize(max: 256, live: [])` binds them by name -- and, as with any
# `**{}`, an empty one is no argument at all, which is what lets that
# override's defaults apply to a bare `P.new`. zeo handed it one positional
# Hash: "wrong number of arguments (given 1, expected 0)".

P = Data.define(:max, :live)
class P
  DEFAULT_MAX = 256
  def initialize(max: DEFAULT_MAX, live: [])
    super(max: max, live: live.freeze)
  end
end
p P.new(max: 320)
p P.new
p P.new(max: 1, live: [2]).live.frozen?
p P.new(320)
p P.new(320, [1])

# The default `initialize` still refuses a short call by keyword name, and a
# memberless Data takes the bare call.
D = Data.define(:a)
begin
  D.new
rescue ArgumentError => e
  p e.message
end
p D.new(**{ a: 1 })
E = Data.define
p E.new
p E.new(**{})
