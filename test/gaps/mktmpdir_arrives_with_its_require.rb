# `Dir.mktmpdir` and `Pathname.mktmpdir` before `require "tmpdir"`.
p Dir.respond_to?(:mktmpdir), Pathname.respond_to?(:mktmpdir)
require "tmpdir"
p Dir.respond_to?(:mktmpdir), Pathname.respond_to?(:mktmpdir)
p Dir.mktmpdir { |d| File.directory?(d) }
__END__
false
false
true
false
true
