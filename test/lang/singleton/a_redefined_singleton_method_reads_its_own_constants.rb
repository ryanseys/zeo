# A method written inside `class << self` and defined again at a second site
# still reads the constants of the singleton body it was written in.
module Exp
  class << self
    class Differ
      PREP = :prep
    end

    def differ
      Differ::PREP
    end
  end
end

p Exp.differ

module Exp
  class << self
    def differ = [:second, Differ::PREP]
  end
end

p Exp.differ

module Exp
  def self.differ = :third
end

p Exp.differ
__END__
:prep
[:second, :prep]
:third
