module Pkg3
  Target = Data.define(:name, :build_block) do
    def call(*a) = build_block.call(*a)
  end
end
require_relative "target/spec"
