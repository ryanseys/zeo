# Pulled in by the deferred require above; reopens the forward-declared
# container (adds a module method) and its nested class via a compact path.
module Outer::Feature
  def self.digest = "sha256"
end

class Outer::Feature::Policy
  def name = "policy"
end
