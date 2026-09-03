# `dup`/`clone` are ordinary overridable Kernel methods in real Ruby --
# the static arm must fall through to Path 1 dispatch when the
# receiver's class defines its own.

class W
  def dup
    "custom"
  end
end

puts W.new.dup
__END__
custom
