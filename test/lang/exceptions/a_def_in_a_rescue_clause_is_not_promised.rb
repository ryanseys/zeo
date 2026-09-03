# The nested walk reaches a `rescue` clause, but reaching it is not the same as
# promising it: the clause runs only if the body raised. The method's name goes
# on record so a static call site stops folding, and reflection still answers
# what actually ran.
class Guarded
  begin
    Integer("1")
  rescue StandardError
    def only_on_failure
      :recovered
    end
  end
end

p Guarded.instance_methods(false)
p Guarded.new.respond_to?(:only_on_failure)

class Recovered
  begin
    Integer("not a number")
  rescue StandardError
    def only_on_failure
      :recovered
    end
  end
end

p Recovered.instance_methods(false)
p Recovered.new.only_on_failure
__END__
[]
false
[:only_on_failure]
:recovered
