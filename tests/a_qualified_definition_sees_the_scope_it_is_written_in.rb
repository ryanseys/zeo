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

# A superclass that names the class being DEFINED and resolves nowhere else is
# ruby's NameError at the definition, not a compile failure -- api_notify has
# its `require "logger"` commented out and ruby raises there too. The class
# must not come into being either: the search that looks for the name may not
# mint a shell of it on the way past.
module ApiNotify
  module ActiveRecord
    class Logger < Logger
      def tag = "x"
    end
  end
end
p ApiNotify::ActiveRecord::Logger.superclass
