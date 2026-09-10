# A `rescue` clause on the def itself, not a `begin`, catches a NoMethodError raised while computing a local.
def show(u)
  label = (u.details || "~")
  puts label
rescue NoMethodError => e
  puts "caught"
end
show(nil)
__END__
caught
