module Bag
  def self.macro(n) = define_method(n) { n }
  def self.[](k) = "Bag.[] #{k}"
  def helper = "helper"
  module_function :helper
end
include Bag
p Bag[:k]
p helper
p Dir["/definitely/no/such/glob/*"]
p({ a: 1 }[:a])
p [10, 20][1]
