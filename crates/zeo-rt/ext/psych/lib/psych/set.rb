# frozen_string_literal: true
module Psych
  # `!!set` loads as one of these. CRuby's is a Hash subclass, so a set
  # answers like the mapping it is written as -- and naming it is what a
  # caller does to permit it (`permitted_classes: [Psych::Set]`).
  class Set < ::Hash
  end
end
