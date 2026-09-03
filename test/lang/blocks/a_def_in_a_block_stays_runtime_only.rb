# `Reach::through_block` is the one restriction the nested walk keeps: a `def`
# written in a block may never run at all, so it is not registered at compile
# time. It still defines itself when the block runs, which is all CRuby
# promises either.
class Looped
  [1].each do
    def defined_by_the_block
      :ran
    end
  end
end

p Looped.instance_methods(false)
p Looped.new.defined_by_the_block

class NeverRuns
  [].each do
    def never_defined
      :unreachable
    end
  end
end

p NeverRuns.instance_methods(false)
p NeverRuns.new.respond_to?(:never_defined)
__END__
[:defined_by_the_block]
:ran
[]
false
