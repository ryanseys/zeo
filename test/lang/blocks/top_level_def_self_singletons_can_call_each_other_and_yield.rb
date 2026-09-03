def self.outer
  "o:#{inner { 1 }}"
end
def self.inner
  yield + 41
end
puts outer
__END__
o:42
