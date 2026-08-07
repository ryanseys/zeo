# The rails shape, in its own file: inside `Store::Adapters`, the bare `Dumper`
# is the name this very class binds -- so it is skipped, and the search
# continues outward to `Store::Dumper`.
module Store
  module Adapters
    class Dumper < Dumper
      def extra = :extra
    end
  end
end
