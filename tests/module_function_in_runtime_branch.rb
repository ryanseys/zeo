# `module_function` mode applies to a `def` inside a RUNTIME-undecidable
# branch when the branch executes -- rspec-support's
# `RubyFeatures.ripper_supported?` sits under `if ripper_requirable`.
# Both halves must exist: the public module method and the private
# instance method an includer calls bare.
module M
  module_function
  ok = begin
    require "definitely_not_a_real_feature"
    true
  rescue LoadError
    false
  end
  if ok
    def pick = "then branch"
  else
    def pick = "else branch"
  end
end
p M.pick
class K
  include M
  def call_pick = pick
end
p K.new.call_pick
begin
  K.new.pick
rescue NoMethodError => e
  puts "explicit receiver: #{e.class}"
end
