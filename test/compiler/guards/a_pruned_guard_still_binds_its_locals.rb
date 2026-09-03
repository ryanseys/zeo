# Ruby's parser creates a local binding for an assignment it never runs, so
# `if false; x = 1; end; p x` answers nil rather than raising NameError. A
# class-body guard zeo decides at compile time is spliced away entirely --
# and the binding has to survive it, because the line after usually reads
# the name. resolv.rb's `DefaultFileName = hosts || '/etc/hosts'` sits right
# under the windows-only `if` that assigns `hosts`.
class Hosts
  if RUBY_PLATFORM =~ /mswin/ || RbConfig::CONFIG["host_os"] =~ /mswin/
    path = "C:/windows/system32/drivers/etc/hosts"
  end
  DEFAULT = path || "/etc/hosts"

  # A name the body already assigned KEEPS its value: the dead branch binds,
  # it does not reset.
  kept = 5
  if RUBY_PLATFORM =~ /mswin/
    kept = 9
  end
  KEPT = kept

  # The `case` fold answers the same way.
  case RbConfig::CONFIG["host_os"]
  when /mswin/ then flavour = :windows
  end
  FLAVOUR = flavour.inspect

  # The branch that IS taken assigns for real.
  if RUBY_PLATFORM =~ /darwin|linux|bsd/
    live = :unix
  else
    live = :other
  end
  LIVE = live
end

p Hosts::DEFAULT
p Hosts::KEPT
p Hosts::FLAVOUR
p Hosts::LIVE
puts "still running"
__END__
"/etc/hosts"
5
"nil"
:unix
still running
