class Version
  include Comparable
  attr_reader :parts
  def initialize(str); @parts = str.split(".").map(&:to_i); end
  def <=>(other); parts <=> other.parts; end
  def to_s; parts.join("."); end
end
versions = ["1.2.0", "1.10.1"].map { |s| Version.new(s) }
puts versions.min
puts versions[0]
puts "#{versions.max}"
p versions.min.to_s
