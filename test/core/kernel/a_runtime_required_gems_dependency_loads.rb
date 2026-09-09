# A `require` reached only from a METHOD BODY loads the vendored gem at
# run time -- but the gem's own nested dependency require does not
# deliver its constants: tempfile's `require "tmpdir"` leaves
# Dir::Tmpname undefined and Tempfile.create raises NameError. The
# top-level-require form works. A cousin of the dual-homed fold in
# `error_highlight_library.rb`.
def go
  require "tempfile"
  Tempfile.create("x") { puts "created" }
end
go
__END__
created
