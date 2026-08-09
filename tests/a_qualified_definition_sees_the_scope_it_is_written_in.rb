# `class A::B` skips the prefix it SPELLS -- its body does not see `A`'s
# constants lexically -- but it keeps every scope it is written inside.
# httpx spells `class Connection::HTTP2` inside `module HTTPX` and then
# `class Error < Error`, meaning `HTTPX::Error`.
module HTTPX
  MAX = 42
  class Error < StandardError; end
  class Connection; end

  class Connection::HTTP2
    LOCAL = MAX + 1

    class Error < Error
      def tag = "h2"
    end
    class PingError < Error; end

    def limit = MAX
  end
end

p HTTPX::Connection::HTTP2::Error.superclass
p HTTPX::Connection::HTTP2::PingError.superclass
p HTTPX::Connection::HTTP2::Error.new.tag
p HTTPX::Connection::HTTP2::LOCAL
p HTTPX::Connection::HTTP2.new.limit
p HTTPX::Connection::HTTP2.name

# The prefix itself stays invisible: `Connection`'s own constants are not in
# the cref, so a bare name that only it defines raises.
module Store
  class Bin
    HIDDEN = :bin
  end
  class Bin::Slot
    def peek
      HIDDEN
    rescue NameError => e
      e.class
    end
  end
end
p Store::Bin::Slot.new.peek

# A top-level qualified definition has nothing enclosing it, so its cref is
# just itself.
TOP = :top
class Store::Loose
  def reach = TOP
end
p Store::Loose.new.reach
