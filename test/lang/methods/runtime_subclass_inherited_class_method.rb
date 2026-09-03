# A runtime subclass (`Class.new(B)`) dispatches an inherited class method
# through the nearest REGISTERED ancestor's materialized copy -- whose
# self-sends were re-resolved from that ancestor's viewpoint -- not the
# owner's own row. B overrides `leaf`, so `tmpl` through the runtime
# subclass must see B's override, the same answer the compiled `B.tmpl`
# call gets (minitest's Runnable/Test split is this exact shape).
class A
  def self.tmpl = "tmpl:#{leaf}"
  def self.leaf = (raise NotImplementedError, "subclass responsibility")
end
class B < A
  def self.leaf = "B-leaf"
end
p B.tmpl
k = Class.new(B)
p k.tmpl
__END__
"tmpl:B-leaf"
"tmpl:B-leaf"
