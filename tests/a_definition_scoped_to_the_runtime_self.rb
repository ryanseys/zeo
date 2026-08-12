# `class self::Task` / `module self::Base` name their NAMESPACE with the
# runtime `self`, so there is no compile-time class to register: the
# constant lands on whichever class the enclosing hook block runs against.
# dk-dumpdb gives every including script its own `Task` subclass this way,
# and flight_facade's `module self::Base` is the ActiveSupport::Concern
# spelling of the same idea.
module Task
  def run = "ran"
end

module Script
  def self.hooks = (@hooks ||= [])
  def self.on_included(&blk) = hooks << blk
  def self.included(base) = hooks.each { |b| base.class_eval(&b) }

  on_included do
    class self::Task
      include Task

      def self.owner_tag = :task
    end

    module self::Helpers
      def self.tag = :helper
    end

    # A superclass expression, and the constant read back in the same body.
    class self::Timed < self::Task
      def run = "timed " + super
    end
  end
end

class Alpha
  include Script
end

class Beta
  include Script
end

p Alpha::Task.new.run
p Alpha::Task.ancestors.include?(Task)
p Alpha::Task.name
p Alpha::Task.owner_tag
p Alpha::Helpers.tag
p Alpha::Timed.new.run
p Alpha::Timed.superclass == Alpha::Task
# Each includer gets its OWN pair -- the whole point of scoping to `self`.
p Beta::Task.name
p Beta::Task.equal?(Alpha::Task)
puts "still running"
