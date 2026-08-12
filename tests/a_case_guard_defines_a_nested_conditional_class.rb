module Host
end
def engine = "native"
case engine
when "java"
  module Host
    class Backend
      def id = "j"
    end
  end
else
  module Host
    class Backend
      def id = "n"
    end
  end
end
puts Host.constants.sort.inspect
puts Host::Backend.new.id
puts defined?(Host::Backend)
h = Host::Backend
puts h.instance_method(:id).owner
puts h.new.respond_to?(:id)
