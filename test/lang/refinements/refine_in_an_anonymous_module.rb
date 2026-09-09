# `refine` is a private method of `Module`, so every module has it -- including
# one built with `Module.new`. zeo defines it only for a module introduced by a
# `module` KEYWORD, so the anonymous form raises
# `NoMethodError: undefined method 'refine'`.
#
# Building the refinement holder at run time is how a library parameterizes one:
# a `Module.new { refine(target) { ... } }` inside a method is the only way to
# refine a class chosen by the caller, since the `module` keyword needs a
# constant name written in the source.
#
# `test/lang/refinements/issue_runtime_refinement_module.rb` covers what happens after a
# runtime refinement is DEFINED. This is the step before it: the definition is
# refused outright.

m = Module.new do
  refine(String) do
    def shout = upcase + "!"
  end
end
p m.class
p m.refinements.size

module Named
  refine String do
    def whisper = downcase
  end
end
p Named.refinements.size

# Not activated here, so both must still be absent.
begin
  "x".shout
rescue NoMethodError => e
  puts "not active: #{e.class}"
end
__END__
Module
1
1
not active: NoMethodError
