# Sibling/top-level resolution runs BEFORE the Kernel function set --
# real Ruby's rule (a user `def rand` wins over Kernel#rand).

def rand
  7
end
puts rand
__END__
7
