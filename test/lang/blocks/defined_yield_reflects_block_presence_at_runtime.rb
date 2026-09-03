# defined?(yield) is nil without a block, "yield" with one.

def f
  defined?(yield)
end
p f
p f { 1 }
def g(&blk)
  defined?(yield)
end
p g
p g { 42 }
__END__
nil
"yield"
nil
"yield"
