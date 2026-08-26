module Store
  class Spec
    def self.limit = Rule::LIMIT
  end
  # A method-body `require_relative` is LAZY, so zeo splices it at the file
  # tail -- and the file it reaches requires `rule.rb` at top level. That
  # trailing splice used to claim the autoload's slot.
  def self.late
    require_relative "cmd"
  end
end
