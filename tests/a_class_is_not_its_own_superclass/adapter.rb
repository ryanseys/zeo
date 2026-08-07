# ... and the adapters that inherit THROUGH it, each in its own file, which is
# what turns one bad edge into a chain every later pass walks.
module Store
  module Adapters
    class MySQL < Adapters::Dumper
      def kind = [:mysql, super]
    end
  end
end
