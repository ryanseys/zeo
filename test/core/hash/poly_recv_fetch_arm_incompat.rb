# Sibling to poly_recv_fetch_hash_arms.rb. A user class defines `fetch` with
# a Symbol key, and a call site elsewhere calls `fetch` with a String key on
# a receiver that is always a Hash. The user class cannot be the receiver
# there, so its definition must not decide how the String call is compiled.

class FlashLike
  def fetch(key, default)
    @last = key
    default
  end
end

# Sym-only direct callers commit FlashLike#fetch's `key` to sym
# (mrb_int) in isolation. The dispatch site below mustn't widen it.
def warmup
  f = FlashLike.new
  f.fetch(:a, "1")
  f.fetch(:b, "2")
end

warmup

def get_raw(h); h["x"]; end

def lookup(h)
  sub = get_raw(h)
  sub.fetch("title", "")
end

puts lookup({ "x" => { "title" => "real" } })
__END__
real
