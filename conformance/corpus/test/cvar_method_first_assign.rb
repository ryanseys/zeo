class C
  def self.set; @@x = 5; end
  def self.get; @@x; end
end
C.set
p C.get
