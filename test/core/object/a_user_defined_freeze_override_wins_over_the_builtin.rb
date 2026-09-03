# `freeze`/`frozen?` are ordinary overridable Kernel methods in real
# Ruby -- a class's own definition must win over the universal dispatch.

class Custom
  def freeze
    :custom
  end
end
puts Custom.new.freeze
__END__
custom
